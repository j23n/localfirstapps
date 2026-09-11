package ui

import (
	"crypto/sha256"
	"encoding/hex"
	"io/fs"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"testing"
)

// Pinned hashes of embedded UI bytes. A change is either a deliberate
// vendor bump or tampering — both should fail CI until this table updates.
var assetSHA256 = map[string]string{
	"assets/vendor/chart.umd.min.js":     "2b2af847724edfc0b4c5adee025310eecc1489c4ea53a30ba77145817bd2032f",
	"assets/app.css":                     "e12053ec0a73a2a67dac6a2eb830d46273a4e3968fcd4b5ec66f44261c615820",
	"assets/app.js":                      "abbb00e53cdde36d675e6b6d989519c51fda468b9eda62eacfd8c884c42b5eea",
	"assets/fonts/recursive-latin.woff2": "91bd1462a6351f893fe240c2c2579e0e68689c2e010311862ce04e6df9332153",
}

func TestEmbeddedAssetHashes(t *testing.T) {
	for name, want := range assetSHA256 {
		b, err := assetFS.ReadFile(name)
		if err != nil {
			t.Fatalf("%s: %v", name, err)
		}
		sum := sha256.Sum256(b)
		got := hex.EncodeToString(sum[:])
		if want == "" {
			t.Errorf("%s sha256=%s (record this in assetSHA256)", name, got)
			continue
		}
		if got != want {
			t.Errorf("%s hash %s want %s (tamper or unexpected vendor bump)", name, got, want)
		}
	}
}

func TestEmbeddedAssetsNoNetworkURLs(t *testing.T) {
	re := regexp.MustCompile(`https?://`)
	err := fs.WalkDir(assetFS, ".", func(path string, d fs.DirEntry, err error) error {
		if err != nil || d.IsDir() {
			return err
		}
		b, err := assetFS.ReadFile(path)
		if err != nil {
			return err
		}
		if re.Find(b) != nil {
			t.Errorf("%s contains http(s) URL", path)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	// templates too — they are embedded
	err = fs.WalkDir(tplFS, ".", func(path string, d fs.DirEntry, err error) error {
		if err != nil || d.IsDir() {
			return err
		}
		b, err := tplFS.ReadFile(path)
		if err != nil {
			return err
		}
		if re.Find(b) != nil {
			t.Errorf("%s contains http(s) URL", path)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestCSSCoversCometAndLaptop(t *testing.T) {
	b, err := assetFS.ReadFile("assets/app.css")
	if err != nil {
		t.Fatal(err)
	}
	css := string(b)
	if !strings.Contains(css, "comet ~380px") {
		t.Fatal("css should document 380px comet width")
	}
	if !strings.Contains(css, "@media (min-width: 600px)") {
		t.Fatal("css should enhance at 600px (laptop)")
	}
	if !strings.Contains(css, "--touch: 44px") {
		t.Fatal("touch targets")
	}
}

func TestOnDiskVendorMatchesEmbed(t *testing.T) {
	// guard against editing files on disk and forgetting embed
	for name := range assetSHA256 {
		disk, err := os.ReadFile(filepath.Join(".", strings.TrimPrefix(name, "")))
		if err != nil {
			t.Fatal(err)
		}
		emb, err := assetFS.ReadFile(name)
		if err != nil {
			t.Fatal(err)
		}
		if string(disk) != string(emb) {
			t.Errorf("%s: disk != embed", name)
		}
	}
}
