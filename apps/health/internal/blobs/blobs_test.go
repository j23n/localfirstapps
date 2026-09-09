package blobs

import (
	"bytes"
	"os"
	"path/filepath"
	"testing"
)

const helloSHA = "a70940623490fa4c251737cf74e1bf75a0327bb18766cc5620edbde3a985c96d"

func TestPutGoldenHello(t *testing.T) {
	src := filepath.Join("..", "..", "testdata", "blobs", "hello.txt")
	b, err := os.ReadFile(src)
	if err != nil {
		t.Fatal(err)
	}
	root := t.TempDir()
	hash, n, existed, err := Put(root, bytes.NewReader(b))
	if err != nil {
		t.Fatal(err)
	}
	if existed {
		t.Fatal("first put existed")
	}
	if hash != helloSHA {
		t.Fatalf("hash=%s want %s", hash, helloSHA)
	}
	if n != 40 {
		t.Fatalf("size=%d", n)
	}
	p, err := Path(root, hash)
	if err != nil {
		t.Fatal(err)
	}
	got, err := os.ReadFile(p)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(got, b) {
		t.Fatal("stored bytes differ")
	}
	_, _, existed, err = Put(root, bytes.NewReader(b))
	if err != nil {
		t.Fatal(err)
	}
	if !existed {
		t.Fatal("second put should exist")
	}
	fi, err := os.Stat(p)
	if err != nil {
		t.Fatal(err)
	}
	if fi.Mode().Perm() != 0o600 {
		t.Fatalf("blob file mode %o", fi.Mode().Perm())
	}
	di, err := os.Stat(filepath.Dir(p))
	if err != nil {
		t.Fatal(err)
	}
	if di.Mode().Perm() != 0o700 {
		t.Fatalf("blob dir mode %o", di.Mode().Perm())
	}
}

func TestPathRejectsBadHash(t *testing.T) {
	if _, err := Path("/tmp", "abcd"); err == nil {
		t.Fatal("accepted short hash")
	}
}

func TestListAndVerifyFixture(t *testing.T) {
	root := filepath.Join("..", "..", "testdata", "m0")
	listed, err := List(root)
	if err != nil {
		t.Fatal(err)
	}
	if len(listed) != 1 || listed[0].SHA256 != helloSHA {
		t.Fatalf("list: %+v", listed)
	}
	if err := Verify(root, helloSHA); err != nil {
		t.Fatal(err)
	}
}
