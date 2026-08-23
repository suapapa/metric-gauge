package main

import (
	"context"
	"time"

	"github.com/shirou/gopsutil/v3/cpu"
	"github.com/shirou/gopsutil/v3/mem"
)

type Metrics struct {
	Cpu float64
	Mem float64
}

func getMetrics(ctx context.Context) chan *Metrics {
	ch := make(chan *Metrics)
	go func(ctx context.Context) {
		tk := time.NewTicker(duration)
		defer tk.Stop()
		for {
			select {
			case <-ctx.Done():
				return
			case <-tk.C:
				v, _ := mem.VirtualMemory()
				c, _ := cpu.PercentWithContext(ctx, 1*time.Second, false)
				ch <- &Metrics{Cpu: c[0], Mem: v.UsedPercent}
			}
		}
	}(ctx)
	return ch
}
