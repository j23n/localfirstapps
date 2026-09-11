package ui

import "embed"

//go:embed assets/app.css assets/app.js assets/vendor/chart.umd.min.js assets/fonts/recursive-latin.woff2
var assetFS embed.FS

//go:embed templates/*.html
var tplFS embed.FS
