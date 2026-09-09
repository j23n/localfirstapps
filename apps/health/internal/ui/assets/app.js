(function () {
  function boot() {
    initTheme();
    applyChartDefaults();
    initCharts();
    bindCards();
    bindAxisToggles();
  }

  function cssVar(name, fallback) {
    var v = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
    return v || fallback;
  }

  function tokens() {
    return {
      bg: cssVar("--bg", "#F2F2F7"),
      surface: cssVar("--surface", "#FFFFFF"),
      ink: cssVar("--ink", "#1C1C1E"),
      inkDim: cssVar("--ink-dim", "#57575C"),
      inkHint: cssVar("--ink-hint", "#8A8A8E"),
      grid: cssVar("--grid", "#E8E8ED"),
      font: cssVar("--font", "-apple-system, BlinkMacSystemFont, system-ui, sans-serif")
    };
  }

  function initTheme() {
    var root = document.documentElement;
    if (!root.getAttribute("data-theme")) {
      try {
        var saved = localStorage.getItem("archive-theme");
        if (saved === "dark" || saved === "light") {
          root.setAttribute("data-theme", saved);
        } else if (window.matchMedia("(prefers-color-scheme: dark)").matches) {
          root.setAttribute("data-theme", "dark");
        } else {
          root.setAttribute("data-theme", "light");
        }
      } catch (e) {
        root.setAttribute("data-theme", "light");
      }
    }
    syncToggle();

    var toggle = document.getElementById("theme-toggle");
    if (toggle && !toggle.getAttribute("data-bound")) {
      toggle.setAttribute("data-bound", "1");
      toggle.addEventListener("click", function () {
        var dark = root.getAttribute("data-theme") === "dark";
        var next = dark ? "light" : "dark";
        root.setAttribute("data-theme", next);
        try { localStorage.setItem("archive-theme", next); } catch (e) {}
        syncToggle();
        applyChartDefaults();
        paintAll();
      });
    }

    try {
      if (!window.matchMedia || localStorage.getItem("archive-theme")) {
        return;
      }
      window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", function (ev) {
        if (localStorage.getItem("archive-theme")) return;
        root.setAttribute("data-theme", ev.matches ? "dark" : "light");
        syncToggle();
        applyChartDefaults();
        paintAll();
      });
    } catch (e) {}
  }

  function syncToggle() {
    var toggle = document.getElementById("theme-toggle");
    if (!toggle) return;
    var dark = document.documentElement.getAttribute("data-theme") === "dark";
    toggle.setAttribute("aria-label", dark ? "Use light theme" : "Use dark theme");
  }

  function applyChartDefaults() {
    if (typeof Chart === "undefined") return;
    var t = tokens();
    Chart.defaults.font.family = t.font;
    Chart.defaults.font.size = 12;
    Chart.defaults.font.weight = "400";
    Chart.defaults.color = t.inkHint;
    Chart.defaults.borderColor = t.grid;
    Chart.defaults.backgroundColor = t.surface;
    Chart.defaults.elements.line.borderWidth = 2;
    Chart.defaults.elements.line.borderJoinStyle = "round";
    Chart.defaults.elements.line.borderCapStyle = "round";
    Chart.defaults.elements.bar.borderRadius = 2;
    Chart.defaults.elements.bar.borderSkipped = false;
  }

  function bindAxisToggles() {
    document.querySelectorAll(".axis-tog").forEach(function (tog) {
      if (tog.getAttribute("data-bound")) return;
      tog.setAttribute("data-bound", "1");
      var panel = tog.closest(".trace-head");
      var host = panel && panel.nextElementSibling ? panel.nextElementSibling.querySelector(".session-chart") : null;
      tog.querySelectorAll("button[data-axis]").forEach(function (btn) {
        btn.addEventListener("click", function () {
          tog.querySelectorAll("button").forEach(function (b) {
            b.classList.toggle("active", b === btn);
          });
          if (!host) return;
          host.setAttribute("data-axis", btn.getAttribute("data-axis"));
          paintCard(host);
        });
      });
      if (host && !host.getAttribute("data-axis")) {
        host.setAttribute("data-axis", "moving");
      }
    });
  }

  function bindCards() {
    document.querySelectorAll("details.card").forEach(function (el) {
      if (el.getAttribute("data-bound")) return;
      el.setAttribute("data-bound", "1");
      el.addEventListener("toggle", function () {
        if (el.open) {
          paintCard(el);
        }
      });
      if (el.open) {
        paintCard(el);
      }
    });
  }

  function paintAll() {
    document.querySelectorAll("details.card[open], .page-chart").forEach(paintCard);
  }

  function initCharts() {
    paintAll();
  }

  function paintCard(card) {
    var node = card.querySelector(".chart-data");
    var canvas = card.querySelector("canvas.chart");
    if (!node || !canvas) {
      return;
    }
    if (typeof Chart === "undefined") {
      canvas.parentNode.setAttribute("data-empty", "Chart library did not load");
      return;
    }
    var nodes = card.querySelectorAll(".chart-data");
    var axis = card.getAttribute("data-axis");
    if (axis) {
      nodes.forEach(function (n) {
        if (n.getAttribute("data-axis") === axis) node = n;
      });
    }
    var cfg;
    try {
      cfg = JSON.parse(node.textContent);
    } catch (e) {
      canvas.parentNode.setAttribute("data-empty", "Chart data was not valid");
      return;
    }
    var t = tokens();
    var kind = (cfg.type || "bar").toLowerCase();
    var last = 0;
    (cfg.datasets || []).forEach(function (ds) {
      if (ds.data && ds.data.length > last) last = ds.data.length;
    });
    last -= 1;
    var lineColor = cfg.color || t.ink;
    var datasets = (cfg.datasets || []).map(function (ds) {
      var out = {
        label: ds.label,
        data: ds.data,
        borderWidth: kind === "line" ? 2 : 0,
        pointRadius: 0,
        pointHitRadius: 18,
        spanGaps: true,
        tension: 0,
        borderJoinStyle: "round",
        borderCapStyle: "round",
        borderRadius: 2,
        borderSkipped: false
      };
      if (kind === "line") {
        out.borderColor = lineColor;
        out.backgroundColor = "transparent";
        out.fill = false;
      } else {
        out.backgroundColor = function (ctx) {
          return ctx.dataIndex === last ? t.ink : t.inkDim;
        };
      }
      return out;
    });
    var existing = Chart.getChart(canvas);
    if (existing) {
      existing.destroy();
    }
    var yTicks = {
      color: t.inkHint,
      font: { family: t.font, size: 11, weight: "400" }
    };
    function fmtNum(v) {
      if (cfg.yFormat === "pace") {
        var sec = Math.round(v);
        if (sec < 0) sec = 0;
        var m = Math.floor(sec / 60);
        var s = sec % 60;
        return m + ":" + (s < 10 ? "0" : "") + s;
      }
      return v;
    }
    if (cfg.yFormat === "pace") {
      yTicks.callback = function (v) {
        return fmtNum(v);
      };
    }
    var yScale = {
      title: cfg.yTitle ? { display: true, text: cfg.yTitle, color: t.inkHint, font: { family: t.font, size: 11, weight: "400" } } : { display: false },
      ticks: yTicks,
      grid: { color: t.grid, drawBorder: false },
      border: { display: false }
    };
    if (cfg.beginAtZero) {
      yScale.beginAtZero = true;
    }
    new Chart(canvas, {
      type: kind,
      data: { labels: cfg.labels || [], datasets: datasets },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        events: ["click", "touchstart", "touchmove", "mousemove"],
        interaction: { mode: "nearest", intersect: false },
        datasets: {
          bar: { minBarLength: cfg.rangeBars ? 2 : 0 }
        },
        plugins: {
          legend: { display: false },
          tooltip: {
            enabled: true,
            backgroundColor: t.ink,
            titleColor: t.surface,
            bodyColor: t.surface,
            displayColors: false,
            callbacks: {
              label: function (ctx) {
                var raw = ctx.raw;
                if (raw == null) return "";
                var name = ctx.dataset.label || "";
                var mid = (ctx.dataset.center || [])[ctx.dataIndex];
                if (Array.isArray(raw)) {
                  var lo = raw[0], hi = raw[1];
                  var span = fmtNum(lo) + "–" + fmtNum(hi);
                  if (mid != null && lo !== hi) {
                    return name + " " + fmtNum(mid) + " · " + span;
                  }
                  return name + " " + span;
                }
                return name + " " + fmtNum(raw);
              }
            }
          }
        },
        scales: {
          x: {
            title: cfg.xTitle ? { display: true, text: cfg.xTitle, color: t.inkHint, font: { family: t.font, size: 11, weight: "400" } } : { display: false },
            ticks: {
              color: t.inkHint,
              maxRotation: 0,
              autoSkip: true,
              maxTicksLimit: 8,
              font: { family: t.font, size: 11, weight: "400" }
            },
            grid: { display: false },
            border: { display: false }
          },
          y: yScale
        }
      }
    });
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", boot);
  } else {
    boot();
  }
})();
