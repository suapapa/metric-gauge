package main

import (
	"context"
	"fmt"
	"io"
	"log"
	"net/http"
	"strconv"
	"strings"
	"time"
)

type cpuSnapshot struct {
	total float64
	idle  float64
}

func getNodeExporterMetrics(ctx context.Context, endpoint string) chan *Metrics {
	ch := make(chan *Metrics)
	go func(ctx context.Context) {
		client := &http.Client{Timeout: 30 * time.Second}
		tk := time.NewTicker(duration)
		defer tk.Stop()

		var prev *cpuSnapshot
		for {
			select {
			case <-ctx.Done():
				return
			case <-tk.C:
				m, err := scrapeNodeExporter(ctx, client, endpoint, prev)
				if err != nil {
					log.Println(err)
					continue
				}
				if m.cpuSnap != nil {
					prev = m.cpuSnap
				}
				ch <- &Metrics{Cpu: m.cpu, Mem: m.mem}
			}
		}
	}(ctx)
	return ch
}

type scrapedMetrics struct {
	cpu     float64
	mem     float64
	cpuSnap *cpuSnapshot
}

func scrapeNodeExporter(
	ctx context.Context,
	client *http.Client,
	endpoint string,
	prev *cpuSnapshot,
) (*scrapedMetrics, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return nil, err
	}

	resp, err := client.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("node-exporter: HTTP %s", resp.Status)
	}

	body, err := io.ReadAll(io.LimitReader(resp.Body, 8<<20))
	if err != nil {
		return nil, err
	}

	text := string(body)
	cur := parseCpuSnapshot(text)
	mem, ok := parseMemPercent(text)
	if !ok {
		return nil, fmt.Errorf("node-exporter: memory metrics not found")
	}

	out := &scrapedMetrics{mem: mem, cpuSnap: &cur}
	if prev != nil {
		if cpu, ok := cpuPercent(cur, *prev); ok {
			out.cpu = cpu
		}
	}
	return out, nil
}

func parseCpuSnapshot(body string) cpuSnapshot {
	var snap cpuSnapshot
	for _, line := range strings.Split(body, "\n") {
		if !strings.HasPrefix(line, "node_cpu_seconds_total") {
			continue
		}
		v, err := parsePromValue(line)
		if err != nil {
			continue
		}
		snap.total += v
		if strings.Contains(line, `mode="idle"`) {
			snap.idle += v
		}
	}
	return snap
}

func parseMemPercent(body string) (float64, bool) {
	var total, available float64
	var hasTotal, hasAvail bool

	for _, line := range strings.Split(body, "\n") {
		switch {
		case strings.HasPrefix(line, "node_memory_MemTotal_bytes"):
			if v, err := parsePromValue(line); err == nil {
				total = v
				hasTotal = true
			}
		case strings.HasPrefix(line, "node_memory_MemAvailable_bytes"):
			if v, err := parsePromValue(line); err == nil {
				available = v
				hasAvail = true
			}
		}
	}

	if !hasTotal || !hasAvail || total == 0 {
		return 0, false
	}
	return 100 * (1 - available/total), true
}

func cpuPercent(current, prev cpuSnapshot) (float64, bool) {
	dTotal := current.total - prev.total
	dIdle := current.idle - prev.idle
	if dTotal <= 0 {
		return 0, false
	}
	return 100 * (1 - dIdle/dTotal), true
}

func parsePromValue(line string) (float64, error) {
	i := strings.LastIndexAny(line, " \t")
	if i < 0 {
		return 0, fmt.Errorf("no metric value in %q", line)
	}
	return strconv.ParseFloat(strings.TrimSpace(line[i+1:]), 64)
}
