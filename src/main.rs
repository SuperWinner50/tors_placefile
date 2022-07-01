use chrono::{DateTime, Datelike, Utc};
use lazy_static::lazy_static;
use regex::Regex;
use std::collections::BTreeMap;
use std::io::{Cursor, Write};
use std::str::FromStr;
use tiny_http::{Request, Response, Server, StatusCode};

/// An http error that will be returned as a response.
#[derive(Debug)]
enum HttpError {
    NotFound,
    BadRequest,
    ParseError(<hyper::Uri as FromStr>::Err),
    GetError(hyper::Error),
    ToBytesError(hyper::Error),
    Utf8Error(std::string::FromUtf8Error),
}

type HttpResult<T> = Result<T, HttpError>;

/// Helper trait to convert tuple of result into result.
trait TupIntoResult<T, E> {
    fn into_result(self) -> Result<(T, T), E>;
}

impl<T, E> TupIntoResult<T, E> for (Result<T, E>, Result<T, E>) {
    fn into_result(self) -> Result<(T, T), E> {
        Ok((self.0?, self.1?))
    }
}

/// Parse queries for a link.
fn parse_params(string: &str) -> HttpResult<BTreeMap<String, String>> {
    lazy_static! {
        static ref URL_PARSE: Regex = Regex::new(r"(?:\?|&)([^&=]+)=([^&=]+)").unwrap();
    }

    let captures: Vec<_> = URL_PARSE
        .captures_iter(string)
        .map(|cap| {
            (
                cap.get(1)
                    .ok_or(HttpError::BadRequest)
                    .map(|c| c.as_str().to_string()),
                cap.get(2)
                    .ok_or(HttpError::BadRequest)
                    .map(|c| c.as_str().to_string()),
            )
                .into_result()
        })
        .collect::<Result<_, _>>()?;

    Ok(BTreeMap::from_iter(captures.into_iter()))
}

/// Shorthand for conting a timezone-less string and format to a datetime.
fn to_utc(s: &str, fmt: &str) -> HttpResult<DateTime<Utc>> {
    use chrono::naive::{NaiveDate, NaiveDateTime};

    let naive_time = match NaiveDateTime::parse_from_str(s, fmt) {
        Ok(time) => time,
        Err(_) => NaiveDate::parse_from_str(s, fmt).map_err(|_| HttpError::BadRequest)?.and_hms(0, 0, 0),
    };

    Ok(DateTime::from_utc(
        naive_time,
        Utc,
    ))
}

/// Parses a url string and returns the start and end time data as an HttpResult.
fn parse_times(string: &str) -> HttpResult<(DateTime<Utc>, DateTime<Utc>)> {
    let params = parse_params(string)?;
    let (start, end) = (
        params.get("start").ok_or(HttpError::BadRequest)?,
        params.get("end").ok_or(HttpError::BadRequest)?,
    );

    (
        to_utc(start, "%F").map_err(|_| HttpError::BadRequest),
        to_utc(end, "%F").map_err(|_| HttpError::BadRequest),
    ).into_result()
}

/// A macro to either return a static or bytes html response.
macro_rules! response {
    ($status_code:literal, $src:literal) => {{
        let bytes = include_bytes!($src).to_vec();
        Response::new(
            StatusCode($status_code),
            Vec::new(),
            Cursor::new(bytes),
            None,
            None,
        )
    }};

    ($status_code:literal, $bytes:expr) => {{
        Response::new(StatusCode($status_code), Vec::new(), $bytes, None, None)
    }};
}

/// Detects the severity of a warning text, and returns a color string and line width.
fn warning_color(text: &str) -> (&str, f32) {
    if text.contains("EMERGENCY") {
        ("0 0 0", 5.)
    } else if text.contains("PARTICULARLY DANGEROUS SITUATION") {
        ("255 0 255", 4.)
    } else if text.contains("OBSERVED") || text.contains("reported") {
        ("150 0 0", 3.5)
    } else {
        ("255 0 0", 3.)
    }
}

/// Tests if a warning is valid.
fn is_valid(text: &str) -> bool {
    !(text.contains("TEST") || text.len() < 50 || text.contains("404"))
}

/// Finds all warnings in a given range.
fn find_warnings((mut start, end): (DateTime<Utc>, DateTime<Utc>)) -> HttpResult<Vec<u8>> {
    use futures::{stream, StreamExt, TryStreamExt};
    use hyper::{body, client::Client};

    lazy_static! {
        static ref PATH: Regex = Regex::new(r"LAT\.\.\.LON [\d{4}\s]+").unwrap();
        static ref TIME: Regex = Regex::new(r".(\d{6}T\d{4}Z)-").unwrap();
    }

    let mut hours = Vec::new();

    while start <= end {
        let url = format!("https://mesonet.agron.iastate.edu/archive/data/{y}/{m:0>2}/{d:0>2}/text/noaaport/TOR_{y}{m:0>2}{d:0>2}.txt",
            y=start.year(), m=start.month(), d=start.day());
        hours.push(url);
        start = start + chrono::Duration::days(1);
    }

    println!("Reading {} files...", hours.len());

    let https = hyper_tls::HttpsConnector::new();
    let client = &Client::builder().build::<_, hyper::Body>(https);
    let reqs = stream::iter(hours)
        .map(|url| async move {
            client
                .get(url.parse().map_err(HttpError::ParseError)?)
                .await
                .map_err(HttpError::GetError)
        })
        .buffer_unordered(8)
        .and_then(|res| async {
            String::from_utf8(
                body::to_bytes(res)
                    .await
                    .map_err(HttpError::ToBytesError)?
                    .to_vec(),
            )
            .map_err(HttpError::Utf8Error)
        })
        .try_collect::<Vec<String>>();

    let warnings: Vec<String> = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(reqs)?
        .into_iter()
        .flat_map(|text| text.split("$$").map(|s| s.to_owned()).collect::<Vec<_>>())
        .filter(|text| is_valid(text))
        .collect();

    let mut writer = Vec::new();
    writeln!(&mut writer, "Title: Past TORs\nRefresh: 9999\n").unwrap();

    for warning in warnings {
        let mut path: Vec<f32> = PATH
            .find(&warning)
            .unwrap_or_else(|| panic!("No path found: {warning}"))
            .as_str()
            .split_whitespace()
            .skip(1)
            .map(|v| v.parse::<f32>().unwrap() / 100.)
            .collect();

        let time = to_utc(
            TIME.captures(&warning)
                .unwrap()
                .get(1)
                .expect("Time parsing error")
                .as_str(),
            "%y%m%dT%H%MZ",
        )?
        .format("%c")
        .to_string();

        let (color, width) = warning_color(&warning);

        path.push(path[0]);
        path.push(path[1]);

        writeln!(
            &mut writer,
            "Color: {color}\nLine: {width}, 0, \"Issued {time}\""
        )
        .unwrap();
        for co in path.chunks_exact(2) {
            writeln!(&mut writer, "{}, {}", co[0], -co[1]).unwrap()
        }
        writeln!(&mut writer, "End:\n").unwrap();
    }

    println!("Done.");

    Ok(writer)
}

/// Handles a request.
fn handle_request(request: Request) {
    let is_correct = request
        .url()
        .starts_with("/warnings.txt")
        .then_some(Vec::<u8>::new())
        .ok_or(HttpError::NotFound);

    let result = is_correct
        .and(parse_times(request.url()))
        .and_then(find_warnings);

    let response = match result {
        Ok(bytes) => response!(200, Cursor::new(bytes)),
        Err(HttpError::NotFound) => response!(404, "not-found.html"),
        Err(HttpError::BadRequest) => response!(400, "bad-request.html"),
        Err(e) => {
            eprintln!("An unexpected error occured: {:?}", e);
            response!(500, "server-error.html")
        }
    };

    request.respond(response).unwrap();
}

fn main() {
    let server = Server::http("localhost:8888").unwrap();
    for request in server.incoming_requests() {
        handle_request(request);
    }
}
