# israel-weather-rs
gets weather forecast xml from ims.gov.il and parses it into rust structs

Runs on my android phone to alert me when it's likely to rain in the next 6 hours via an integration with https://llamalab.com/automate/. Could also be setup to alert you or run on linux/mac/windows/raspberry pi with another notification wrapper like https://github.com/nikoksr/notify or https://github.com/caronc/apprise

## Help
```
$ weather --help
Downloads and Caches Israeli weather forecast from https://ims.gov.il and prints the next forecast for a location as json

Usage: weather [OPTIONS]

Options:
  -l, --location <LOCATION>  Location to check weather for [default: "Tel Aviv - Yafo"]
  -n, --next <NEXT>          Check next n hours ahead [default: 6]
  -a, --all                  Ignore location and print all weather data
  -h, --help                 Print help
  -V, --version              Print version
```

## Example output
```json
[
  {
    "DewPointTemp": 5,
    "FeelsLike": 15,
    "ForecastTime": "2023-02-12T14:00:00+00:00",
    "HeatStress": 12,
    "HeatStressLevel": 0,
    "MaxTemp": 15,
    "MinTemp": 12,
    "Rain": 0.25,
    "RelativeHumidity": 54,
    "Temperature": 15,
    "UvIndex": null,
    "UvIndexMax": null,
    "WeatherCode": 1530,
    "WeatherCodeEnglish": "Partly cloudy possible rain",
    "WindChill": 14,
    "WindDirection": 113,
    "WindSpeed": 13
  }
]
```

## Installation
Download from the latest release, or if you need rpi/android from the most recently built cross-compile action

## Get Started with Dev
1. Get rust via rustup
1. cargo run
1. profit

## Running on Android: with help from llamalab automate
I used https://llamalab.com/automate/ [Google Play Store link](https://play.google.com/store/apps/details?id=com.llamalab.automate&referrer=utm_source%3Dhomepage) to run the Android build of this on my android phone on a schedule in order to notify me of expected upcoming rain even when my phone is offline.

I use the [termux/termux-tasker](https://github.com/termux/termux-tasker) [plugin in llamalabs automate](https://llamalab.com/automate/doc/block/plugin_setting.html) to run the latest Android release on a schedule, and to use the Speak and Notifications blocks of Automate.

The [flow file](./barakplasma_israel-weather-rs.flo) can be imported in the Automate app after you setup termux-tasker with it's permissions.