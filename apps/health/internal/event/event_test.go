package event

import "testing"

func TestValidateUTC(t *testing.T) {
	ev := Event{
		ID:   "01900000-0000-7000-8000-000000000001",
		TS:   "2024-01-15T12:00:00.000000000Z",
		Dev:  "manual",
		Type: TypeNote,
		Body: []byte(`{"text":"ok"}`),
	}
	if err := ev.Validate(); err != nil {
		t.Fatal(err)
	}
	ev.TS = "2024-01-15T12:00:00+00:00"
	if err := ev.Validate(); err == nil {
		t.Fatal("accepted offset timestamp")
	}
	ev.TS = "2024-01-15T12:00:00.000000000Z"
	ev.Type = "not_a_type"
	if err := ev.Validate(); err == nil {
		t.Fatal("accepted unknown type")
	}
}

func TestValidateTSPrecision(t *testing.T) {
	ev := Event{
		ID:   "01900000-0000-7000-8000-000000000001",
		TS:   "2024-01-15T12:00:00.000000000Z",
		Dev:  "manual",
		Type: TypeNote,
		Body: []byte(`{"text":"ok"}`),
	}
	if err := ev.Validate(); err != nil {
		t.Fatalf("fixture-precision ts rejected: %v", err)
	}
	for _, ts := range []string{"2024-01-15T12:00:00Z", "2024-01-15T12:00:00.5Z"} {
		ev.TS = ts
		if err := ev.Validate(); err == nil {
			t.Fatalf("accepted short ts %q", ts)
		}
	}
}

func TestDeviceRejectsTraversal(t *testing.T) {
	for _, name := range []string{"", "..", "a/b", "a\\b", " foo"} {
		if ValidDevice(name) {
			t.Fatalf("accepted %q", name)
		}
	}
	if !ValidDevice("instinct-1") {
		t.Fatal("rejected instinct-1")
	}
}

func TestParseBlobImportAliases(t *testing.T) {
	b, err := ParseBlobImport([]byte(`{"blob":"A70940623490FA4C251737CF74E1BF75A0327BB18766CC5620EDBDE3A985C96D","orig_filename":"hello.txt"}`))
	if err != nil {
		t.Fatal(err)
	}
	if b.SHA256 != "a70940623490fa4c251737cf74e1bf75a0327bb18766cc5620edbde3a985c96d" {
		t.Fatalf("sha256=%s", b.SHA256)
	}
	if b.Name != "hello.txt" {
		t.Fatalf("name=%s", b.Name)
	}
	ev := Event{Body: []byte(`{"blob":"a70940623490fa4c251737cf74e1bf75a0327bb18766cc5620edbde3a985c96d"}`)}
	hash, ok := ev.BlobSHA256()
	if !ok || hash != b.SHA256 {
		t.Fatalf("BlobSHA256=%s ok=%v", hash, ok)
	}
}

func TestMarshalFieldOrder(t *testing.T) {
	ev := Event{
		ID:   "01900000-0000-7000-8000-000000000001",
		TS:   "2024-01-15T12:00:00.000000000Z",
		Dev:  "manual",
		Type: TypeNote,
		Body: []byte(`{"text":"ok"}`),
	}
	line, err := ev.MarshalLine()
	if err != nil {
		t.Fatal(err)
	}
	want := `{"id":"01900000-0000-7000-8000-000000000001","ts":"2024-01-15T12:00:00.000000000Z","dev":"manual","type":"note","body":{"text":"ok"}}` + "\n"
	if string(line) != want {
		t.Fatalf("got %s", line)
	}
}
