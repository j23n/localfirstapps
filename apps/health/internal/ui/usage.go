package ui

import (
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"sync"

	"archive/internal/event"
)

const maxUsageLog = 4 << 20

// UsageLogPath is the local-only view log under derived/.
func UsageLogPath(root string) string {
	return filepath.Join(root, "derived", "uiusage.log")
}

type usageLogger struct {
	path string
	mu   sync.Mutex
}

func (u *usageLogger) log(path, rawQuery string) {
	if u == nil || u.path == "" {
		return
	}
	if strings.HasPrefix(path, "/assets/") {
		return
	}
	line := event.NowUTC() + "\t" + path + "\t" + rawQuery + "\n"
	u.mu.Lock()
	defer u.mu.Unlock()
	if err := os.MkdirAll(filepath.Dir(u.path), 0o700); err != nil {
		return
	}
	if fi, err := os.Stat(u.path); err == nil && fi.Size() > maxUsageLog {
		_ = os.Remove(u.path + ".1")
		if err := os.Rename(u.path, u.path+".1"); err != nil {
			return
		}
	}
	f, err := os.OpenFile(u.path, os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0o600)
	if err != nil {
		return
	}
	_ = f.Chmod(0o600)
	_, _ = f.WriteString(line)
	_ = f.Close()
}

func (u *usageLogger) middleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		u.log(r.URL.Path, r.URL.RawQuery)
		next.ServeHTTP(w, r)
	})
}
