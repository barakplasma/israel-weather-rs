// Package compat checks that third-party Open-Meteo clients can use `weather serve`.
// Run against a live server: WEATHER_BASE_URL=http://127.0.0.1:8080 go test ./...
package compat

import (
	"context"
	"os"
	"testing"
	"time"

	"github.com/hectormalot/omgo"
)

// Tel Aviv.
const lat, lon = 32.08, 34.78

func client(t *testing.T) *omgo.Client {
	base := os.Getenv("WEATHER_BASE_URL")
	if base == "" {
		t.Skip("WEATHER_BASE_URL not set")
	}
	return omgo.NewClient(omgo.WithForecastURL(base + "/v1/forecast"))
}

func ctx(t *testing.T) context.Context {
	c, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	t.Cleanup(cancel)
	return c
}

func TestOmgoHourlyDailyCurrent(t *testing.T) {
	req, err := omgo.NewForecastRequest(lat, lon)
	if err != nil {
		t.Fatal(err)
	}
	req.WithHourly(
		omgo.HourlyTemperature2m, omgo.HourlyApparentTemperature, omgo.HourlyRelativeHumidity2m,
		omgo.HourlyPrecipitation, omgo.HourlyPrecipitationProbability, omgo.HourlyWeatherCode,
		omgo.HourlyWindSpeed10m, omgo.HourlyWindGusts10m, omgo.HourlyIsDay,
	).WithDaily(
		omgo.DailyTemperature2mMax, omgo.DailyTemperature2mMin, omgo.DailyPrecipitationSum,
		omgo.DailyPrecipitationProbabilityMax, omgo.DailyWeatherCode,
	).WithCurrent(
		omgo.CurrentTemperature2m, omgo.CurrentWeatherCode, omgo.CurrentIsDay,
	).WithTimezone("Asia/Jerusalem").WithForecastDays(3)

	w, err := client(t).Forecast(ctx(t), req)
	if err != nil {
		t.Fatalf("forecast: %v", err)
	}
	if w.Timezone != "Asia/Jerusalem" || w.UTCOffsetSeconds < 7200 {
		t.Errorf("timezone %q offset %d", w.Timezone, w.UTCOffsetSeconds)
	}
	h := w.Hourly
	if h == nil || len(h.Times) != 72 {
		t.Fatalf("want 72 hourly times, got %+v", h)
	}
	if len(h.Temperature2m) != len(h.Times) || len(h.WeatherCode) != len(h.Times) {
		t.Errorf("series lengths differ: %d temps, %d codes, %d times",
			len(h.Temperature2m), len(h.WeatherCode), len(h.Times))
	}
	if loc := h.Times[0].Location().String(); loc != "Asia/Jerusalem" {
		t.Errorf("times parsed in %s", loc)
	}
	if t0 := h.Temperature2m[30]; t0 < -10 || t0 > 50 {
		t.Errorf("implausible temperature %v", t0)
	}
	if w.HourlyUnits == nil || w.HourlyUnits.Temperature2m != "°C" {
		t.Errorf("hourly units %+v", w.HourlyUnits)
	}
	d := w.Daily
	if d == nil || len(d.Times) != 3 || len(d.Temperature2mMax) != 3 {
		t.Fatalf("daily %+v", d)
	}
	if d.Temperature2mMax[0] < d.Temperature2mMin[0] {
		t.Errorf("max %v < min %v", d.Temperature2mMax[0], d.Temperature2mMin[0])
	}
	if c := w.Current; c == nil || c.Temperature2m == nil || c.WeatherCode == nil || c.IsDay == nil {
		t.Errorf("current %+v", w.Current)
	} else if c.WeatherCode.String() == "" {
		t.Errorf("unknown WMO code %v", *c.WeatherCode)
	}
}

func TestOmgoUnits(t *testing.T) {
	req, _ := omgo.NewForecastRequest(lat, lon)
	req.WithHourly(omgo.HourlyTemperature2m, omgo.HourlyWindSpeed10m, omgo.HourlyPrecipitation).
		WithTemperatureUnit(omgo.Fahrenheit).
		WithWindSpeedUnit(omgo.MilesPerHour).
		WithPrecipitationUnit(omgo.Inches).
		WithForecastDays(1)
	w, err := client(t).Forecast(ctx(t), req)
	if err != nil {
		t.Fatalf("forecast: %v", err)
	}
	u := w.HourlyUnits
	if u.Temperature2m != "°F" || u.WindSpeed10m != "mp/h" || u.Precipitation != "inch" {
		t.Errorf("units %+v", u)
	}
}

func TestOmgoBadRequestIsAnAPIError(t *testing.T) {
	req, _ := omgo.NewForecastRequest(lat, lon)
	req.WithTimezone("Mars/Olympus_Mons").WithHourly(omgo.HourlyTemperature2m)
	if _, err := client(t).Forecast(ctx(t), req); err == nil {
		t.Fatal("want an error for an invalid timezone")
	}
}
