package main

import (
	"flag"
	"fmt"
	"log"
	"net/http"
	"os"

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
)

func init() {
	flag.IntVar(&flagPort, "port", 8080, "HTTP(S) listen port")
	flag.StringVar(&flagPrinter, "printer", "", "Printer name (default: system default)")
	flag.BoolVar(&flagTLS, "tls", false, "Enable HTTPS with self-signed cert")
	flag.StringVar(&flagCertFile, "cert", "", "TLS certificate file (PEM)")
	flag.StringVar(&flagKeyFile, "key", "", "TLS private key file (PEM)")
	flag.BoolVar(&flagVerbose, "verbose", false, "Verbose logging (hex dumps)")
	flag.BoolVar(&flagSelftest, "selftest", false, "Run self-test via spooler and exit")
	flag.BoolVar(&flagDrawer, "drawer", false, "Enable drawer kick (ESC p)")
	flag.IntVar(&flagPaperWidth, "paper-width", 576, "Paper width in dots (384=58mm, 512/576=80mm)")
	flag.StringVar(&flagCodepage, "codepage", "CP437", "Barcode/QR codepage")
	flag.StringVar(&flagLogFile, "log-file", "", "Log file path (rotating, max 10MB)")
	flag.Usage = func() {
		fmt.Fprintf(os.Stderr, "ePOS Printer Emulator for Odoo Online POS\n\n")
		fmt.Fprintf(os.Stderr, "Usage: %s [flags]\n\n", os.Args[0])
		flag.PrintDefaults()
	}
}

func main() {
	flag.Parse()
	defer logging.Close()

	// Initialize logging (T12)
	logging.Init(flagVerbose, flagLogFile)

	if flagSelftest {
		os.Exit(runSelftest())
	}

	handler := eposhttp.Handler(flagPrinter, flagVerbose, flagDrawer)
	addr := fmt.Sprintf(":%d", flagPort)

	log.Printf("ePOS Emulator listening on %s (tls=%v)", addr, flagTLS)
	log.Printf("Printer: %q, Paper: %d dots, Drawer: %v", flagPrinter, flagPaperWidth, flagDrawer)

	// Load or generate TLS cert (T5)
	if flagTLS && flagCertFile == "" {
		flagCertFile = "cert.pem"
		flagKeyFile = "key.pem"
		if err := tlshelper.GenerateSelfSigned(flagCertFile, flagKeyFile); err != nil {
			log.Fatalf("TLS cert generation failed: %v", err)
		}
		log.Printf("Generated self-signed cert: %s / %s", flagCertFile, flagKeyFile)
	}

	var err error
	if flagTLS {
		err = http.ListenAndServeTLS(addr, flagCertFile, flagKeyFile, handler)
	} else {
		err = http.ListenAndServe(addr, handler)
	}

	if err != nil {
		log.Fatalf("Server error: %v", err)
	}
}

func runSelftest() int {
	fmt.Println("ePOS Emulator Self-Test")

	data := escpos.SelfTestBytes()
	fmt.Printf("ESC/POS bytes (%d): %x\n", len(data), data)

	// Try to send to printer on Windows
	hh, err := winspool.OpenPrinter(flagPrinter)
	if err != nil {
		fmt.Printf("NOTE: Spooler unavailable on this platform: %v\n", err)
		fmt.Println("Self-test passed (dry run).")
		return 0
	}
	defer winspool.ClosePrinter(hh)

	if err := winspool.PrintRaw(hh, "ePOS Self-Test", data); err != nil {
		fmt.Printf("ERROR: PrintRaw failed: %v\n", err)
		return 1
	}
	fmt.Println("Self-test sent to printer successfully.")
	return 0
}
