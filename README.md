# tors_placefile
A program to convert archived tornado warnings into placefiles.

## Installation
- Install [Rust](https://www.rust-lang.org/)
- Clone this respository to any folder

## Usage
- Run `cargo run --release` inside the downloaded folder
- Connect to `http://localhost:8888/warnings.txt` using the syntax below

## Parameter syntax
The start and end times can be set using the `start` and `end` parameters.
Any requests not following this syntax will result in an error.

Example: `http://localhost:8888/warnings.txt?start=2022-05-01&end=2022-06-01`

## Color codes
- Red: Radar indicated
- Dark red: Tornado observed or reported
- Pink: PDS
- Black: Tornado emergency

## Extra
All data used here is provided by IEM, accessible [here](https://mesonet.agron.iastate.edu/archive/data).
Please be aware that this program may be intensive on their servers if the requests are too large, so be careful.