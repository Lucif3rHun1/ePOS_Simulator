package main

import (
	"flag"
	"fmt"
	"net/http"
	"os"
	"os/signal"
	"syscall"

	"epos-emulator/internal/eposhttp"
	"epos-emulator/internal/escpos"
	"epos-emulator/internal/logging"
	"epos-emulator/internal/tls"
	"epos-emulator/internal/winspool"
)

var (
	flagPort        int
	flagPrinter     string
	flagTLS         bool
	flagCertFile    string
	flagKeyFile     string
	flagVerbose     bool
	flagSelftest    bool
	flagDrawer      bool
	flagPaperWidth  int
	flagCodepage    string
	flagLogFile     string
	flagListPrinters bool
	flagMaxLogBytes int64
	flagMaxBackups  int
)

func init() {
	flag.IntVar(&flagPort, "port", 8080, "HTTP(S) listen port")
	flag.StringVar(&flagPrinter, "printer", "", "Printer name (use --list-printers to see options; empty=system default)")
	flag.BoolVar(&flagTLS, "tls", false, "Enable HTTPS with self-signed cert")
	flag.StringVar(&flagCertFile, "cert", "", "TLS certificate file (PEM)")
	flag.StringVar(&flagKeyFile, "key", "", "TLS private key file (PEM)")
	flag.BoolVar(&flagVerbose, "verbose", false, "Verbose logging (hex dumps of XML + ESC/POS)")
	flag.BoolVar(&flagSelftest, "selftest", false, "Run self-test via spooler and exit")
	flag.BoolVar(&flagDrawer, "drawer", false, "Enable drawer kick (ESC p)")
	flag.IntVar(&flagPaperWidth, "paper-width", 576, "Paper width in dots (384=58mm, 512/576=80mm)")
	flag.StringVar(&flagCodepage, "codepage", "CP437", "Barcode/QR codepage")
	flag.StringVar(&flagLogFile, "log-file", "", "Log file path (rotating, default 10MB x 3 backups)")
	flag.BoolVar(&flagListPrinters, "list-printers", false, "List installed printers and exit")
	flag.Int64Var(&flagMaxLogBytes, "max-log-bytes", 10*1024*1024, "Log rotation size threshold in bytes")
	flag.IntVar(&flagMaxBackups, "max-log-backups", 3, "Number of rotated log backups to keep (.1, .2, .N)")
	flag.Usage = func() {
		fmt.Fprintf(os.Stderr, "ePOS Printer Emulator for Odoo Online POS\n\n")
		fmt.Fprintf(os.Stderr, "Usage: %s [flags]\n\n", os.Args[0])
		flag.PrintDefaults()
		fmt.Fprintf(os.Stderr, "\nExamples:\n")
		fmt.Fprintf(os.Stderr, "  %s --list-printers\n", os.Args[0])
		fmt.Fprintf(os.Stderr, "  %s --port 8080 --printer \"EPSON TM-T20\"\n", os.Args[0])
		fmt.Fprintf(os.Stderr, "  %s --tls --verbose --log-file epos.log\n", os.Args[0])
		fmt.Fprintf(os.Stderr, "  %s --selftest --printer \"EPSON TM-T20\"\n", os.Args[0])
	}
}

func main() {
	flag.Parse()

	logging.Init(logging.Config{
		Verbose:    flagVerbose,
		LogFile:    flagLogFile,
		MaxBytes:   flagMaxLogBytes,
		MaxBackups: flagMaxBackups,
		IncludePID: true,
	})
	defer logging.Close()

	logging.Info("starting",
		"version", "1.0.0",
		"platform", logging.Platform(),
		"verbose", flagVerbose,
		"log_file", flagLogFile,
	)

	if flagListPrinters {
		os.Exit(runListPrinters())
	}

	if flagSelftest {
		os.Exit(runSelftest())
	}

	// Trap SIGINT/SIGTERM for graceful shutdown.
	sigCh := make(chan os.Signal, 1)
	signal.Notify(sigCh, syscall.SIGINT, syscall.SIGTERM)
	go func() {
		sig := <-sigCh
		logging.Info("shutting down", "signal", sig.String())
		logging.Close()
		os.Exit(0)
	}()

	handler := eposhttp.Handler(flagPrinter, flagVerbose, flagDrawer)
	addr := fmt.Sprintf(":%d", flagPort)

	if flagPrinter == "" {
		logging.Info("using system default printer")
	} else {
		logging.Info("printer selected", "name", flagPrinter)
	}
	logging.Info("listening",
		"addr", addr,
		"tls", flagTLS,
		"paper_width", flagPaperWidth,
		"drawer_enabled", flagDrawer,
	)

	// Load or generate TLS cert (T5)
	if flagTLS && flagCertFile == "" {
		flagCertFile = "cert.pem"
		flagKeyFile = "key.pem"
		if err := tlshelper.GenerateSelfSigned(flagCertFile, flagKeyFile); err != nil {
			logging.Error("tls cert generation failed", "err", err.Error())
			os.Exit(1)
		}
		logging.Info("generated self-signed cert", "cert", flagCertFile, "key", flagKeyFile)
	}

	var err error
	if flagTLS {
		logging.Info("https mode", "addr", addr, "cert", flagCertFile)
		err = http.ListenAndServeTLS(addr, flagCertFile, flagKeyFile, handler)
	} else {
		logging.Info("http mode", "addr", addr)
		err = http.ListenAndServe(addr, handler)
	}

	if err != nil {
		logging.Error("server error", "err", err.Error())
		os.Exit(1)
	}
}

func runListPrinters() int {
	infos, err := winspool.EnumPrinters()
	if err != nil {
		fmt.Fprintf(os.Stderr, "ERROR: %v\n", err)
		return 1
	}
	fmt.Print(winspool.FormatList(infos))
	logging.Info("list-printers completed", "count", len(infos))
	return 0
}

func runSelftest() int {
	logging.Info("selftest starting")
	fmt.Println("ePOS Emulator Self-Test")

	data := escpos.SelfTestBytes()
	fmt.Printf("ESC/POS bytes (%d): %x\n", len(data), data)

	// Try to send to printer on Windows
	hh, err := winspool.OpenPrinter(flagPrinter)
	if err != nil {
		fmt.Printf("NOTE: Spooler unavailable on this platform: %v\n", err)
		logging.Info("selftest dry run (non-Windows or no printer)", "err", err.Error())
		fmt.Println("Self-test passed (dry run).")
		return 0
	}
	defer winspool.ClosePrinter(hh)

	if err := winspool.PrintRaw(hh, "ePOS Self-Test", data); err != nil {
		fmt.Printf("ERROR: PrintRaw failed: %v\n", err)
		logging.Error("selftest printraw failed", "err", err.Error())
		return 1
	}
	fmt.Println("Self-test sent to printer successfully.")
	logging.Info("selftest passed")
	return 0
}
