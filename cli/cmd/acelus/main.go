package main

import (
	"fmt"
	"os"
)

var version = "0.1.0"

func main() {
	if len(os.Args) > 1 && os.Args[1] == "version" {
		fmt.Printf("acelus %s\n", version)
		return
	}
	fmt.Fprintln(os.Stderr, "usage: acelus <command>")
	os.Exit(2)
}
