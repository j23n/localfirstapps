package ui

import (
	"database/sql"
	"fmt"
	"html/template"
	"net/http"
	"time"

	"archive/config"
	"archive/internal/projection"
)

type page struct {
	Title        string
	Nonce        string
	View         string
	Offset       string
	On           string
	Prev         string
	Next         string
	Back         string
	BackLabel    string
	Kinds        []projection.KindInfo
	Cards        []kindCard
	Card         kindCard
	Favs         []kindCard
	Groups       []domainGroup
	Workouts     []workoutView
	Workout      workoutView
	Session      sessionView
	WantWorkouts bool
	Unclass      []kindCard
	Empty        string
	Gaps         *gapsData
	Blobs        []projection.BlobImportRow
	GapLine      string
	ChartJSON    template.JS
}

type domainGroup struct {
	Domain string
	Label  string
	Hue    string
	Cards  []kindCard
}

type gapsData struct {
	Overall  projection.GapReport
	Kind     *projection.GapReport
	Selected string
	PerKind  []projection.GapReport
	Sample   []string
}

func (s *Server) open() (*sql.DB, error) {
	return projection.Open(s.Root)
}

func (s *Server) today(p *page, r *http.Request) error {
	return s.dayPage(p, r, "")
}

func (s *Server) medicine(p *page, r *http.Request) error {
	p.View = "medicine"
	p.Title = "Medicine"
	return s.domainPage(p, r, config.DomainMedicine)
}

func (s *Server) lifestyle(p *page, r *http.Request) error {
	p.View = "lifestyle"
	p.Title = "Lifestyle"
	return s.domainPage(p, r, config.DomainLifestyle)
}

func (s *Server) sports(p *page, r *http.Request) error {
	p.View = "sports"
	p.Title = "Sports"
	return s.domainPage(p, r, config.DomainSports)
}

func (s *Server) dayPage(p *page, r *http.Request, domain string) error {
	db, err := s.open()
	if err != nil {
		p.Empty = "No projection yet. Import a source and rebuild."
		return nil
	}
	defer db.Close()
	on := r.URL.Query().Get("on")
	if on == "" {
		on, err = projection.LatestLocalDay(db, s.loc)
		if err != nil {
			return err
		}
	}
	if on == "" {
		p.Empty = "No observations in the projection."
		return nil
	}
	t, err := time.ParseInLocation("2006-01-02", on, s.loc)
	if err != nil {
		return fmt.Errorf("day: %w", err)
	}
	p.On = on
	p.Prev = t.AddDate(0, 0, -1).Format("2006-01-02")
	p.Next = t.AddDate(0, 0, 1).Format("2006-01-02")
	p.Title = "Today"

	g, err := projection.OverallGaps(db, s.loc)
	if err != nil {
		return err
	}
	p.GapLine = fmt.Sprintf("%d days missing between %s and %s", g.MissingN(), g.First, g.Last)

	cat, err := projection.KindCatalog(db)
	if err != nil {
		return err
	}
	byKind := map[string]projection.KindInfo{}
	for _, k := range cat {
		byKind[k.Kind] = k
	}

	win, err := projection.LocalWindow(on, "", "", s.loc)
	if err != nil {
		return err
	}
	dayRows, err := projection.ListObservations(db, projection.ObsQuery{Window: win})
	if err != nil {
		return err
	}
	present := map[string]bool{}
	for _, row := range dayRows {
		present[row.Kind] = true
	}

	favSet := map[string]bool{}
	for _, k := range config.Favourites() {
		favSet[k] = true
		info := byKind[k]
		if info.Kind == "" {
			info.Kind = k
		}
		c, err := s.loadDayCard(db, info, on)
		if err != nil {
			return err
		}
		p.Favs = append(p.Favs, c)
	}

	grouped := map[string][]kindCard{}
	for _, info := range cat {
		if info.Table != "observations" {
			continue
		}
		if favSet[info.Kind] {
			continue
		}
		if domain == "" && !present[info.Kind] {
			continue
		}
		sp := config.Lookup(info.Kind)
		if domain != "" && sp.Domain != domain {
			continue
		}
		c, err := s.loadDayCard(db, info, on)
		if err != nil {
			return err
		}
		grouped[sp.Domain] = append(grouped[sp.Domain], c)
	}
	order := []string{config.DomainMedicine, config.DomainLifestyle, config.DomainSports, config.DomainOther}
	labels := map[string]string{
		config.DomainMedicine:  "Medicine",
		config.DomainLifestyle: "Lifestyle",
		config.DomainSports:    "Sports",
		config.DomainOther:     "Unclassified",
	}
	for _, d := range order {
		if domain != "" && d != domain {
			continue
		}
		if len(grouped[d]) == 0 {
			continue
		}
		p.Groups = append(p.Groups, domainGroup{Domain: d, Label: labels[d], Hue: domainHue(d), Cards: grouped[d]})
	}

	p.WantWorkouts = config.FavouriteWorkouts()
	eps, err := projection.ListEpisodes(db, projection.EpQuery{Window: win})
	if err != nil {
		return err
	}
	for _, ep := range eps {
		if config.IsWorkoutKind(ep.Kind) {
			p.Workouts = append(p.Workouts, parseWorkout(ep, func(ts string) string {
				d, _ := localDay(ts, s.loc)
				return d
			}))
		}
	}
	return nil
}

func (s *Server) domainPage(p *page, r *http.Request, domain string) error {
	db, err := s.open()
	if err != nil {
		if domain == config.DomainMedicine {
			p.Empty = "No lab or medication data yet. Those arrive in later milestones. This page is the shell."
		} else {
			p.Empty = "No projection yet. Import a source and rebuild."
		}
		return nil
	}
	defer db.Close()
	cat, err := projection.KindCatalog(db)
	if err != nil {
		return err
	}
	var cards []kindCard
	for _, info := range cat {
		if config.Lookup(info.Kind).Domain != domain {
			continue
		}
		c, err := s.loadCatalogCard(db, info)
		if err != nil {
			return err
		}
		cards = append(cards, c)
	}
	p.Cards = cards
	if len(cards) == 0 {
		if domain == config.DomainMedicine {
			p.Empty = "No lab or medication data yet. Apple’s export does not include the medication log. This band is ready for M5/M6."
		} else {
			p.Empty = "No observations in the projection."
		}
	}
	return nil
}

func (s *Server) allKinds(p *page, r *http.Request) error {
	p.Title = "All"
	db, err := s.open()
	if err != nil {
		p.Empty = "No projection yet."
		return nil
	}
	defer db.Close()
	cat, err := projection.KindCatalog(db)
	if err != nil {
		return err
	}
	p.Kinds = cat
	grouped := map[string][]kindCard{}
	for _, info := range cat {
		sp := config.Lookup(info.Kind)
		c, err := s.loadCatalogCard(db, info)
		if err != nil {
			return err
		}
		d := sp.Domain
		if sp.Fallback {
			d = config.DomainOther
		}
		grouped[d] = append(grouped[d], c)
	}
	order := []string{config.DomainMedicine, config.DomainLifestyle, config.DomainSports, config.DomainOther}
	labels := map[string]string{
		config.DomainMedicine:  "Medicine",
		config.DomainLifestyle: "Lifestyle",
		config.DomainSports:    "Sports",
		config.DomainOther:     "Unclassified",
	}
	for _, d := range order {
		if len(grouped[d]) == 0 {
			continue
		}
		p.Groups = append(p.Groups, domainGroup{Domain: d, Label: labels[d], Hue: domainHue(d), Cards: grouped[d]})
	}
	return nil
}

func (s *Server) kindPage(p *page, r *http.Request) error {
	return s.fillKind(p, r, "", r.URL.Query().Get("k"))
}

func (s *Server) kindFromPath(domain string) func(*page, *http.Request) error {
	return func(p *page, r *http.Request) error {
		return s.fillKind(p, r, domain, r.PathValue("slug"))
	}
}

func (s *Server) fillKind(p *page, r *http.Request, domain, q string) error {
	if q == "" {
		q = r.URL.Query().Get("kind")
	}
	if q == "" {
		p.Empty = "Pick a kind from a section."
		p.Title = "Kind"
		p.View = "all"
		p.Back = "/all"
		p.BackLabel = "All"
		return nil
	}
	db, err := s.open()
	if err != nil {
		p.Empty = "No projection yet. Import a source and rebuild."
		p.Title = "Kind"
		p.View = "all"
		p.Back = "/all"
		p.BackLabel = "All"
		return nil
	}
	defer db.Close()
	cat, err := projection.KindCatalog(db)
	if err != nil {
		return err
	}
	info, ok := matchKindSlug(cat, domain, q)
	if !ok {
		prefer := "observations"
		if domain == config.DomainSports {
			prefer = "episodes"
		}
		resolved, err := projection.ResolveKindInDB(db, q, prefer)
		if err != nil {
			if domain != "" {
				return errNotFound
			}
			p.Empty = "Could not resolve that kind."
			p.Title = "Kind"
			p.View = "all"
			p.Back = "/all"
			p.BackLabel = "All"
			return nil
		}
		for _, ki := range cat {
			if ki.Kind == resolved.Kind {
				info = ki
				ok = true
				break
			}
		}
		if !ok {
			info.Kind = resolved.Kind
			if resolved.Episodes && !resolved.Observations {
				info.Table = "episodes"
			} else {
				info.Table = "observations"
			}
		}
		if domain != "" && kindSection(info.Kind) != domain {
			return errNotFound
		}
	}
	sp := config.Lookup(info.Kind)
	p.Title = sp.Display
	p.View, p.Back, p.BackLabel = sectionNav(sp)
	c, err := s.loadDetailCard(db, info, r.URL.Query().Get("source"), r.URL.Query().Get("range"), r.URL.Query().Get("end"))
	if err != nil {
		return err
	}
	p.Card = c
	if config.IsWorkoutKind(info.Kind) {
		p.Workouts, err = s.listWorkouts(db, info.Kind, c.Range, c.End)
		if err != nil {
			return err
		}
	}
	return nil
}

func (s *Server) sessionFromPath(domain string) func(*page, *http.Request) error {
	return func(p *page, r *http.Request) error {
		return s.fillSession(p, r, domain, r.PathValue("slug"), r.PathValue("id"))
	}
}

func (s *Server) fillSession(p *page, r *http.Request, domain, slug, id string) error {
	db, err := s.open()
	if err != nil {
		return errNotFound
	}
	defer db.Close()
	cat, err := projection.KindCatalog(db)
	if err != nil {
		return err
	}
	info, ok := matchKindSlug(cat, domain, slug)
	if !ok || !config.IsWorkoutKind(info.Kind) {
		return errNotFound
	}
	ep, err := projection.EpisodeByKey(db, id)
	if err != nil {
		if err == sql.ErrNoRows {
			return errNotFound
		}
		return err
	}
	if ep.Kind != info.Kind {
		return errNotFound
	}
	evs, err := projection.ListWorkoutEvents(db, ep.DedupKey)
	if err != nil {
		return err
	}
	start, _ := time.Parse(time.RFC3339Nano, ep.Start)
	end, _ := time.Parse(time.RFC3339Nano, ep.End)
	var hr []projection.ObservationRow
	if !start.IsZero() && !end.IsZero() {
		win := projection.TimeWindow{FromUTC: start.UTC(), ToUTC: end.UTC()}
		hr, err = projection.ListObservations(db, projection.ObsQuery{
			Kind:   "HKQuantityTypeIdentifierHeartRate",
			Source: ep.Source,
			Window: win,
		})
		if err != nil {
			return err
		}
	}
	sess := s.buildSession(ep, evs, hr)
	p.Session = sess
	p.Workout = sess.workoutView
	p.Title = sess.Display
	p.View, _, _ = sectionNav(config.Lookup(info.Kind))
	p.Back = kindPath(info.Kind)
	p.BackLabel = sess.Display
	p.On = sess.Day
	return nil
}

func sectionNav(sp config.Spec) (view, back, label string) {
	switch sp.Domain {
	case config.DomainMedicine:
		return "medicine", "/medicine", "Medicine"
	case config.DomainLifestyle:
		return "lifestyle", "/lifestyle", "Lifestyle"
	case config.DomainSports:
		return "sports", "/sports", "Sports"
	default:
		return "all", "/all", "All"
	}
}

func (s *Server) gaps(p *page, r *http.Request) error {
	db, err := s.open()
	if err != nil {
		return nil
	}
	defer db.Close()
	cat, err := projection.KindCatalog(db)
	if err != nil {
		return err
	}
	p.Kinds = cat
	overall, err := projection.OverallGaps(db, s.loc)
	if err != nil {
		return err
	}
	gd := &gapsData{Overall: overall}
	sel := r.URL.Query().Get("kind")
	if sel != "" {
		resolved, err := projection.ResolveKindInDB(db, sel, "observations")
		if err == nil && resolved.Kind != "" {
			sel = resolved.Kind
		}
		g, err := projection.ObservationGaps(db, sel, s.loc)
		if err != nil {
			return err
		}
		gd.Kind = &g
		gd.Selected = sel
		gd.Sample = clip(g.Missing, 40)
	} else {
		gd.Sample = clip(overall.Missing, 40)
	}
	for _, k := range cat {
		if k.Table != "observations" {
			continue
		}
		g, err := projection.ObservationGaps(db, k.Kind, s.loc)
		if err != nil {
			return err
		}
		gd.PerKind = append(gd.PerKind, g)
	}
	p.Gaps = gd
	return nil
}

func (s *Server) blobs(p *page, r *http.Request) error {
	db, err := s.open()
	if err != nil {
		return nil
	}
	defer db.Close()
	rows, err := projection.ListBlobImports(db)
	if err != nil {
		return err
	}
	p.Blobs = rows
	return nil
}

func clip(in []string, n int) []string {
	if len(in) <= n {
		return in
	}
	return in[:n]
}
