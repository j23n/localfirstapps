package ui

import (
	"context"
	"crypto/rand"
	"encoding/base64"
	"errors"
	"fmt"
	"html/template"
	"io/fs"
	"log"
	"net"
	"net/http"
	"strings"
	"time"

	"archive/config"
	"archive/internal/projection"
)

var errNotFound = errors.New("not found")

type ctxKey int

const nonceCtxKey ctxKey = 1

// Server is the read-only local UI.
type Server struct {
	Root string
	loc  *time.Location
	log  *usageLogger
	fn   template.FuncMap
}

// New opens a UI server for an archive root.
func New(root string) (*Server, error) {
	loc, err := projection.DisplayLocation()
	if err != nil {
		return nil, err
	}
	s := &Server{
		Root: root,
		loc:  loc,
		log:  &usageLogger{path: UsageLogPath(root)},
	}
	s.fn = template.FuncMap{
		"display":  config.DisplayOf,
		"hue":      categoryOf,
		"icon":     categoryIcon,
		"baseline": baselineSide,
		"date":     shortDate,
		"longdate": func(on string) string {
			t, err := time.ParseInLocation("2006-01-02", on, s.loc)
			if err != nil {
				return on
			}
			return t.Format("Monday 2 January")
		},
		"timeofday": func(ts string) string {
			t, err := time.Parse(time.RFC3339Nano, ts)
			if err != nil {
				return ts
			}
			return t.In(s.loc).Format("15:04")
		},
		"urlquery": template.URLQueryEscaper,
	}
	return s, nil
}

func shortDate(ts string) string {
	if len(ts) >= 10 {
		return ts[:10]
	}
	return ts
}

// Handler returns the read-only mux.
func (s *Server) Handler() http.Handler {
	mux := http.NewServeMux()
	sub, err := fs.Sub(assetFS, "assets")
	if err != nil {
		panic(err)
	}
	mux.Handle("/assets/", noIndex(http.StripPrefix("/assets/", http.FileServer(http.FS(sub)))))
	mux.HandleFunc("/", s.page("today", s.today))
	mux.HandleFunc("/medicine", s.page("domain", s.medicine))
	mux.HandleFunc("/lifestyle", s.page("domain", s.lifestyle))
	mux.HandleFunc("/sports", s.page("domain", s.sports))
	mux.HandleFunc("/all", s.page("all", s.allKinds))
	mux.HandleFunc("/medicine/{slug}", s.page("kind", s.kindFromPath(config.DomainMedicine)))
	mux.HandleFunc("/lifestyle/{slug}", s.page("kind", s.kindFromPath(config.DomainLifestyle)))
	mux.HandleFunc("/sports/{slug}", s.page("kind", s.kindFromPath(config.DomainSports)))
	mux.HandleFunc("/all/{slug}", s.page("kind", s.kindFromPath("all")))
	mux.HandleFunc("/sports/{slug}/{id}", s.page("session", s.sessionFromPath(config.DomainSports)))
	mux.HandleFunc("/all/{slug}/{id}", s.page("session", s.sessionFromPath("all")))
	mux.HandleFunc("/kind", s.page("kind", s.kindPage))
	mux.HandleFunc("/gaps", s.page("gaps", s.gaps))
	mux.HandleFunc("/blobs", s.page("blobs", s.blobs))
	mux.HandleFunc("/kinds", func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, "/all?"+r.URL.RawQuery, http.StatusFound)
	})
	mux.HandleFunc("/day", func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, "/?"+r.URL.RawQuery, http.StatusFound)
	})
	mux.HandleFunc("/timeline", func(w http.ResponseWriter, r *http.Request) {
		q := r.URL.Query()
		kind := q.Get("kind")
		if kind == "" {
			kind = q.Get("k")
		}
		if kind == "" {
			http.Redirect(w, r, "/all", http.StatusFound)
			return
		}
		q.Del("kind")
		q.Del("k")
		target := kindPath(kind)
		if enc := q.Encode(); enc != "" {
			target += "?" + enc
		}
		http.Redirect(w, r, target, http.StatusFound)
	})
	return localOnly(withSecurityHeaders(s.log.middleware(mux)))
}

func noIndex(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/assets" || strings.HasSuffix(r.URL.Path, "/") {
			http.NotFound(w, r)
			return
		}
		next.ServeHTTP(w, r)
	})
}

func hostName(host string) string {
	if h, _, err := net.SplitHostPort(host); err == nil {
		return h
	}
	return host
}

func localOnly(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		h := hostName(r.Host)
		if h != "127.0.0.1" && h != "localhost" {
			http.Error(w, "forbidden", http.StatusForbidden)
			return
		}
		if strings.EqualFold(r.Header.Get("Sec-Fetch-Site"), "cross-site") {
			http.Error(w, "forbidden", http.StatusForbidden)
			return
		}
		next.ServeHTTP(w, r)
	})
}

func newNonce() string {
	var b [16]byte
	if _, err := rand.Read(b[:]); err != nil {
		log.Println(err)
	}
	return base64.StdEncoding.EncodeToString(b[:])
}

func setSecurityHeaders(h http.Header, nonce string) {
	h.Set("Content-Security-Policy",
		"default-src 'none'; script-src 'self' 'nonce-"+nonce+"'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'")
	h.Set("X-Content-Type-Options", "nosniff")
	h.Set("Referrer-Policy", "no-referrer")
	h.Set("X-Frame-Options", "DENY")
}

func withSecurityHeaders(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		nonce := newNonce()
		setSecurityHeaders(w.Header(), nonce)
		next.ServeHTTP(w, r.WithContext(context.WithValue(r.Context(), nonceCtxKey, nonce)))
	})
}

func requestNonce(r *http.Request) string {
	s, _ := r.Context().Value(nonceCtxKey).(string)
	return s
}

func writeInternal(w http.ResponseWriter, err error) {
	log.Println(err)
	http.Error(w, "internal error", http.StatusInternalServerError)
}

// CheckBind reports whether addr is allowed. Only 127.0.0.1 is accepted.
func CheckBind(addr string) error {
	host, port, err := net.SplitHostPort(addr)
	if err != nil {
		return fmt.Errorf("serve: %w", err)
	}
	if port == "" {
		return fmt.Errorf("serve: missing port")
	}
	if host != "127.0.0.1" {
		return fmt.Errorf("refusing to bind %s; only 127.0.0.1 is allowed", addr)
	}
	return nil
}

// NewServer builds a localhost-only server with request timeouts. It does not bind.
func NewServer(addr string, h http.Handler) (*http.Server, error) {
	if err := CheckBind(addr); err != nil {
		return nil, err
	}
	return &http.Server{
		Addr:              addr,
		Handler:           h,
		ReadHeaderTimeout: 5 * time.Second,
		ReadTimeout:       30 * time.Second,
		WriteTimeout:      60 * time.Second,
		IdleTimeout:       60 * time.Second,
	}, nil
}

// Listen serves h on addr after CheckBind.
func Listen(addr string, h http.Handler) error {
	srv, err := NewServer(addr, h)
	if err != nil {
		return err
	}
	return srv.ListenAndServe()
}

func (s *Server) page(name string, fill func(*page, *http.Request) error) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodGet && r.Method != http.MethodHead {
			http.Error(w, "read-only", http.StatusMethodNotAllowed)
			return
		}
		if name == "today" && r.URL.Path != "/" {
			http.NotFound(w, r)
			return
		}
		title := name
		if name == "today" {
			title = "Today"
		}
		if name == "kind" {
			title = "Kind"
		}
		p := &page{
			Title:  strings.ToUpper(title[:1]) + title[1:],
			View:   name,
			Offset: s.offsetLabel(),
			Nonce:  requestNonce(r),
		}
		if err := fill(p, r); err != nil {
			if errors.Is(err, errNotFound) {
				http.NotFound(w, r)
				return
			}
			writeInternal(w, err)
			return
		}
		t, err := template.New("").Funcs(s.fn).ParseFS(tplFS,
			"templates/layout.html",
			"templates/card.html",
			"templates/workout.html",
			"templates/"+name+".html",
		)
		if err != nil {
			writeInternal(w, err)
			return
		}
		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		w.Header().Set("Cache-Control", "no-store")
		if err := t.ExecuteTemplate(w, name, p); err != nil {
			writeInternal(w, err)
		}
	}
}

func (s *Server) offsetLabel() string {
	return time.Now().In(s.loc).Format("-0700")
}
