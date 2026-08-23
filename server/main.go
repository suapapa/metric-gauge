package main

import (
	"context"
	"flag"
	"fmt"
	"log"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/tarm/serial"
)

var (
	fSerialPort     string
	fIntervalStr    string
	fNodeExporter   string
	duration        time.Duration
)

func main() {
	flag.StringVar(&fSerialPort, "s", "/dev/ttyUSB0", "Serial port to use")
	flag.StringVar(&fIntervalStr, "i", "15s", "Interval to update gauges")
	flag.StringVar(&fNodeExporter, "nodeexporter", "", "node-exporter /metrics URL (disables local gopsutil)")
	flag.Parse()

	log.Println("Opening serial port...")
	gauge, err := serial.OpenPort(&serial.Config{Name: fSerialPort, Baud: 9600})
	if err != nil {
		log.Fatal(err)
	}

	duration, err = time.ParseDuration(fIntervalStr)
	if err != nil {
		log.Fatal(err)
	}

	ctx, cancelF := context.WithCancel(context.Background())
	defer cancelF()

	var mCh chan *Metrics
	if fNodeExporter != "" {
		log.Printf("Using node-exporter endpoint: %s", fNodeExporter)
		mCh = getNodeExporterMetrics(ctx, fNodeExporter)
	} else {
		mCh = getMetrics(ctx)
	}

	log.Println("Updating gauges...")
	go func(ctx context.Context) {
		for {
			select {
			case m := <-mCh:
				log.Printf("cpu %f, mem: %f", m.Cpu, m.Mem)
				if _, err := fmt.Fprintf(
					gauge,
					"A=%d,%d\r\n",
					int(m.Cpu+0.5), int(m.Mem+0.5),
				); err != nil {
					log.Println(err)
				}
			case <-ctx.Done():
				return
			}
		}
	}(ctx)

	c := make(chan os.Signal, 1)
	signal.Notify(c, os.Interrupt, syscall.SIGTERM)
	<-c
	log.Println("Shutting down...")
	cancelF()
	if _, err := fmt.Fprintf(gauge, "A=0,0\r\n"); err != nil {
		log.Println(err)
	}
}

// func scale(v, fromLow, fromHigh, toLow, toHigh float64) float64 {
// 	ret := (v-fromLow)/(fromHigh-fromLow)*(toHigh-toLow) + toLow
// 	if ret < toLow {
// 		ret = toLow
// 	}
// 	if ret > toHigh {
// 		ret = toHigh
// 	}
// 	return ret
// }
