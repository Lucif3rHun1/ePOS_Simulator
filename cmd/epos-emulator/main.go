package main

import (
	"errors"
	"flag"
	"fmt"
	"io"
	"net/http"
	"os"
	"os/signal"
	"strings"
	"syscall"
	"time"

	"epos-emulator/internal/eposhttp"
	"epos-emulator/internal/escpos"
	"epos-emulator/internal/logging"
	"epos-emulator/internal/netinfo"
	"epos-emulator/internal/tls"
	"epos-emulator/internal/winspool"
)

var (
	flagPort         int
	flagPrinter      string
	flagTLS          bool
	flagCertFile     string
	flagKeyFile      string
	flagVerbose      bool
	flagSelftest     bool
	flagDrawer       bool
	flagPaperWidth   int
	flagCodepage     string
	flagLogFile      string
	flagListPrinters bool
	flagMaxLogBytes  int64
	flagMaxBackups   int
	flagStrictXML    bool
	flagInteractive  bool
)

func init() {
	flag.IntVar(&flagPort, "port", 8080, "HTTP(S) listen port")
	flag.StringVar(&flagPrinter, "printer", "", "Printer name (use --list-printers to see options; empty=interactive pick)")
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
	flag.BoolVar(&flagStrictXML, "strict-xml", false, "Reject unknown ePOS-Print XML elements instead of ignoring")
	flag.BoolVar(&flagInteractive, "interactive", true, "When --printer is empty, prompt user to select from enumerated printers")
	flag.Usage = func() {
		fmt.Fprintf(os.Stderr, "ePOS Printer Emulator for Odoo Online POS\n\n")
		fmt.Fprintf(os.Stderr, "Usage: %s [flags]\n\n", os.Args[0])
		flag.PrintDefaults()
		fmt.Fprintf(os.Stderr, "\nExamples:\n")
		fmt.Fprintf(os.Stderr, "  %s --list-printers\n", os.Args[0])
		fmt.Fprintf(os.Stderr, "  %s --port 8080 --printer \"EPSON TM-T20\"\n", os.Args[0])
		fmt.Fprintf(os.Stderr, "  %s --tls --verbose --log-file epos.log\n", os.Args[0])
		fmt.Fprintf(os.Stderr, "  %s --selftest --printer \"EPSON TM-T20\"\n", os.Args[0])
		fmt.Fprintf(os.Stderr, "  %s --strict-xml  # reject unknown elements like odoo/epos-proxy\n", os.Args[0])
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
		"version", "1.1.0",
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

	// Resolve printer name. If --printer is empty and stdin is interactive,
	// list printers and prompt. If non-interactive (CI, piped), fall back to
	// empty = system default.
	if flagPrinter == "" && flagInteractive && winspool.IsInteractive() {
		picked, err := winspool.InteractivePicker{
			In:       os.Stdin,
			Out:      os.Stderr,
			Fallback: "",
		}.Pick()
		if err == nil && picked != "" {
			flagPrinter = picked
			logging.Info("printer picked interactively", "name", picked)
		} else if err != nil && !errors.Is(err, winspool.ErrUserCancelled) {
			logging.Warn("interactive pick failed", "err", err.Error())
		}
	}

	sigCh := make(chan os.Signal, 1)
	signal.Notify(sigCh, syscall.SIGINT, syscall.SIGTERM)
	go func() {
		sig := <-sigCh
		logging.Info("shutting down", "signal", sig.String())
		logging.Close()
		os.Exit(0)
	}()

	handler := eposhttp.Handler(flagPrinter, flagVerbose, flagDrawer, flagStrictXML)
	addr := fmt.Sprintf(":%d", flagPort)
	scheme := "http"
	if flagTLS {
		scheme = "https"
	}

	if flagPrinter == "" {
		logging.Info("using system default printer")
	} else {
		logging.Info("printer selected", "name", flagPrinter)
	}

	printStartupBanner(scheme, addr)

	logging.Info("listening",
		"addr", addr,
		"tls", flagTLS,
		"paper_width", flagPaperWidth,
		"drawer_enabled", flagDrawer,
		"strict_xml", flagStrictXML,
	)

	if flagTLS && flagCertFile == "" {
		flagCertFile = "cert.pem"
		flagKeyFile = "key.pem"
		if err := tlshelper.GenerateSelfSigned(flagCertFile, flagKeyFile); err != nil {
			logging.Error("tls cert generation failed", "err", err.Error())
			os.Exit(1)
		}
		logging.Info("generated self-signed cert", "cert", flagCertFile, "key", flagKeyFile)
	}

	errCh := make(chan error, 1)
	go func() {
		var err error
		if flagTLS {
			logging.Info("https mode", "addr", addr, "cert", flagCertFile)
			err = http.ListenAndServeTLS(addr, flagCertFile, flagKeyFile, handler)
		} else {
			logging.Info("http mode", "addr", addr)
			err = http.ListenAndServe(addr, handler)
		}
		errCh <- err
	}()

	verifyHealth(scheme, addr)

	if err := <-errCh; err != nil {
		logging.Error("server error", "err", err.Error())
		os.Exit(1)
	}
}

func printStartupBanner(scheme, addr string) {
	var sb strings.Builder
	fmt.Fprintln(&sb, "")
	fmt.Fprintln(&sb, "============================================================")
	fmt.Fprintln(&sb, "  ePOS Printer Emulator for Odoo Online POS")
	fmt.Fprintln(&sb, "============================================================")
	fmt.Fprintf(&sb, "  Platform : %s\n", logging.Platform())
	fmt.Fprintf(&sb, "  PID      : %d\n", os.Getpid())
	fmt.Fprintf(&sb, "  Printer  : %s\n", printerLabel())
	fmt.Fprintf(&sb, "  Paper    : %d dots\n", flagPaperWidth)
	fmt.Fprintf(&sb, "  Drawer   : %v\n", flagDrawer)
	fmt.Fprintf(&sb, "  TLS      : %v\n", flagTLS)
	fmt.Fprintf(&sb, "  Verbose  : %v\n", flagVerbose)
	fmt.Fprintf(&sb, "  Strict   : %v\n", flagStrictXML)
	fmt.Fprintf(&sb, "  Log file : %s\n", logFileLabel())
	fmt.Fprintln(&sb, "")
	fmt.Fprintln(&sb, "  API endpoints:")
	fmt.Fprintf(&sb, "    GET  /                          (health check, returns JSON)\n")
	fmt.Fprintf(&sb, "    POST /cgi-bin/epos/service.cgi  (ePOS XML, Odoo POS target)\n")
	fmt.Fprintln(&sb, "")
	fmt.Fprintln(&sb, "  Listening on (port = flag --port):")
	fmt.Fprint(&sb, netinfo.FormatBanner(addr))
	fmt.Fprintln(&sb, "")
	fmt.Fprintln(&sb, "  Odoo POS IoT Printer URL: copy any IP/port above into")
	fmt.Fprintln(&sb, "  Settings > Point of Sale > Printers > Printer IP.")
	fmt.Fprintln(&sb, "============================================================")
	fmt.Fprintln(&sb, "")

	os.Stderr.WriteString(sb.String())
}

func printerLabel() string {
	if flagPrinter == "" {
		return "(system default)"
	}
	return flagPrinter
}

func logFileLabel() string {
	if flagLogFile == "" {
		return "(stderr only)"
	}
	return flagLogFile
}

func verifyHealth(scheme, addr string) {
	time.Sleep(150 * time.Millisecond)
	url := scheme + "://127.0.0.1" + addr + "/"
	client := &http.Client{Timeout: 2 * time.Second}
	resp, err := client.Get(url)
	if err != nil {
		logging.Warn("health check failed", "url", url, "err", err.Error())
		fmt.Fprintf(os.Stderr, "  [WARN] Health check failed: %v\n\n", err)
		return
	}
	defer resp.Body.Close()
	body, _ := io.ReadAll(resp.Body)
	if resp.StatusCode == 200 && strings.Contains(string(body), `"status":"ok"`) {
		logging.Info("health check passed", "url", url, "status", resp.StatusCode)
		fmt.Fprintf(os.Stderr, "  [OK] Health endpoint responds: %s -> %s\n\n", url, strings.TrimSpace(string(body)))
	} else {
		logging.Warn("health check unexpected", "url", url, "status", resp.StatusCode, "body", string(body))
		fmt.Fprintf(os.Stderr, "  [WARN] Health check returned %d: %s\n\n", resp.StatusCode, string(body))
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

	hh, err := winspool.OpenPrinter(flagPrinter)
	if err != nil {
		fmt.Printf("NOTE: Spooler unavailable on this platform: %v\n", err)
		logging.Info("selftest dry run", "err", err.Error())
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
