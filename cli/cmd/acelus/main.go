package main

import (
	"encoding/json"
	"fmt"
	"os"
	"strconv"
	"strings"
	"text/tabwriter"

	"github.com/jaysyrk/Acelus/cli/internal/rpc"
)

const version = "0.1.0"

const usage = `acelus — a Minecraft launcher that authenticates for real

usage:
  acelus login                       sign in with a Microsoft account
  acelus accounts                    list signed in accounts
  acelus accounts use <uuid>         choose the account launches use
  acelus accounts forget <uuid>      sign out and erase stored credentials
  acelus versions [--all] [--limit n]
                                     list published Minecraft versions
  acelus create <name> <version> [--fabric[=version]] [--quilt[=version]]
                                     create an instance
  acelus instances                   list instances
  acelus import [name] [as-name]     bring instances over from Prism or MultiMC
  acelus install <instance>          download and verify everything it needs
  acelus verify <instance>           check an installed instance against its lockfile
  acelus launch <instance>           start the game
  acelus running                     list running sessions
  acelus stop <session>              stop a running session
  acelus logs <session>              print captured output
  acelus delete <instance> [--keep-user-data]
                                     remove an instance
  acelus version                     print the version of this client
`

func main() {
	if len(os.Args) < 2 {
		fmt.Fprint(os.Stderr, usage)
		os.Exit(2)
	}

	if err := run(os.Args[1], os.Args[2:]); err != nil {
		fmt.Fprintf(os.Stderr, "acelus: %s\n", err)
		os.Exit(1)
	}
}

func run(command string, args []string) error {
	switch command {
	case "version", "--version", "-v":
		fmt.Printf("acelus %s\n", version)
		fmt.Println("Copyright (C) 2026 jaysyrk")
		fmt.Println("GPL-3.0-or-later. This program comes with ABSOLUTELY NO WARRANTY.")
		return nil
	case "help", "--help", "-h":
		fmt.Print(usage)
		return nil
	}

	client, err := rpc.DialOrStart(rpc.SocketPath())
	if err != nil {
		return err
	}
	defer client.Close()

	switch command {
	case "login":
		return login(client)
	case "accounts":
		return accounts(client, args)
	case "versions":
		return versions(client, args)
	case "create":
		return create(client, args)
	case "import":
		return importInstances(client, args)
	case "instances":
		return instances(client)
	case "install":
		return install(client, args)
	case "verify":
		return verify(client, args)
	case "launch":
		return launch(client, args)
	case "running":
		return running(client)
	case "stop":
		return stop(client, args)
	case "logs":
		return logs(client, args)
	case "delete":
		return remove(client, args)
	case "daemon":
		return daemon(client)
	default:
		return fmt.Errorf("unknown command %q; run acelus help", command)
	}
}

func requireArg(args []string, index int, what string) (string, error) {
	if len(args) <= index {
		return "", fmt.Errorf("expected %s", what)
	}
	return args[index], nil
}

func table() *tabwriter.Writer {
	return tabwriter.NewWriter(os.Stdout, 0, 0, 2, ' ', 0)
}

func daemon(client *rpc.Client) error {
	var info struct {
		Version    string `json:"version"`
		RPCVersion int    `json:"rpcVersion"`
		DataDir    string `json:"dataDir"`
	}
	if err := client.Call("daemon.info", nil, &info); err != nil {
		return err
	}

	fmt.Printf("acelusd %s (rpc %d)\n", info.Version, info.RPCVersion)
	fmt.Printf("data directory: %s\n", info.DataDir)
	return nil
}

func login(client *rpc.Client) error {
	var begun struct {
		JobID           string `json:"jobId"`
		UserCode        string `json:"userCode"`
		VerificationURI string `json:"verificationUri"`
	}
	if err := client.Call("account.beginLogin", nil, &begun); err != nil {
		return err
	}

	fmt.Printf("Open %s and enter the code %s\n", begun.VerificationURI, begun.UserCode)
	fmt.Println("Waiting for the sign in to complete...")

	var failure error
	err := client.Await(func(method string, params json.RawMessage) {
		if method != "account.loginComplete" {
			return
		}

		var complete struct {
			Account *struct {
				Name                string `json:"name"`
				UUID                string `json:"uuid"`
				EntitlementVerified bool   `json:"entitlementVerified"`
			} `json:"account"`
			Error *rpc.Error `json:"error"`
		}
		if err := json.Unmarshal(params, &complete); err != nil {
			failure = err
			return
		}

		switch {
		case complete.Error != nil:
			failure = complete.Error
		case complete.Account == nil:
			failure = fmt.Errorf("the sign in completed without returning an account")
		default:
			fmt.Printf("Signed in as %s (%s)\n", complete.Account.Name, complete.Account.UUID)
		}
	}, func(method string) bool { return method == "account.loginComplete" })

	if err != nil {
		return err
	}
	return failure
}

func accounts(client *rpc.Client, args []string) error {
	if len(args) > 0 {
		switch args[0] {
		case "use":
			uuid, err := requireArg(args, 1, "an account id")
			if err != nil {
				return err
			}
			return client.Call("account.select", map[string]any{"uuid": uuid}, nil)
		case "forget":
			uuid, err := requireArg(args, 1, "an account id")
			if err != nil {
				return err
			}
			return client.Call("account.remove", map[string]any{"uuid": uuid}, nil)
		default:
			return fmt.Errorf("unknown accounts subcommand %q", args[0])
		}
	}

	var listing struct {
		Active   string `json:"active"`
		Accounts []struct {
			UUID                string `json:"uuid"`
			Name                string `json:"name"`
			EntitlementVerified bool   `json:"entitlementVerified"`
			Expired             bool   `json:"expired"`
		} `json:"accounts"`
	}
	if err := client.Call("account.list", nil, &listing); err != nil {
		return err
	}

	if len(listing.Accounts) == 0 {
		fmt.Println("No accounts signed in. Run: acelus login")
		return nil
	}

	writer := table()
	fmt.Fprintln(writer, "\tNAME\tUUID\tSTATUS")
	for _, account := range listing.Accounts {
		marker := " "
		if account.UUID == listing.Active {
			marker = "*"
		}
		status := "ok"
		if account.Expired {
			status = "expired"
		}
		if !account.EntitlementVerified {
			status = "unverified"
		}
		fmt.Fprintf(writer, "%s\t%s\t%s\t%s\n", marker, account.Name, account.UUID, status)
	}
	return writer.Flush()
}

func versions(client *rpc.Client, args []string) error {
	params := map[string]any{"limit": 25}
	if !contains(args, "--all") {
		params["channels"] = []string{"release"}
	}
	if limit, ok := flagValue(args, "--limit"); ok {
		parsed, err := strconv.Atoi(limit)
		if err != nil {
			return fmt.Errorf("--limit expects a number, got %q", limit)
		}
		params["limit"] = parsed
	}

	var listing struct {
		Versions []struct {
			ID          string `json:"id"`
			Type        string `json:"type"`
			ReleaseTime string `json:"releaseTime"`
		} `json:"versions"`
		Latest struct {
			Release  string `json:"release"`
			Snapshot string `json:"snapshot"`
		} `json:"latest"`
	}
	if err := client.Call("version.list", params, &listing); err != nil {
		return err
	}

	writer := table()
	fmt.Fprintln(writer, "VERSION\tTYPE\tRELEASED")
	for _, entry := range listing.Versions {
		released, _, _ := strings.Cut(entry.ReleaseTime, "T")
		fmt.Fprintf(writer, "%s\t%s\t%s\n", entry.ID, entry.Type, released)
	}
	if err := writer.Flush(); err != nil {
		return err
	}

	fmt.Printf("\nlatest release: %s, latest snapshot: %s\n",
		listing.Latest.Release, listing.Latest.Snapshot)
	return nil
}

func create(client *rpc.Client, args []string) error {
	name, err := requireArg(args, 0, "an instance name")
	if err != nil {
		return err
	}
	wanted, err := requireArg(args, 1, "a Minecraft version")
	if err != nil {
		return err
	}

	params := map[string]any{"name": name, "version": wanted}
	loader, err := loaderSpec(args)
	if err != nil {
		return err
	}
	if loader != nil {
		params["loader"] = loader
	}

	var created struct {
		Instance struct {
			ID   string `json:"id"`
			Path string `json:"path"`
		} `json:"instance"`
	}
	if err := client.Call("instance.create", params, &created); err != nil {
		return err
	}

	fmt.Printf("Created %s at %s\n", created.Instance.ID, created.Instance.Path)
	fmt.Printf("Next: acelus install %s\n", created.Instance.ID)
	return nil
}

type foreignInstance struct {
	Path      string `json:"path"`
	Name      string `json:"name"`
	Version   string `json:"version"`
	BlockedBy string `json:"blockedBy"`
	Loader    *struct {
		Kind    string `json:"kind"`
		Version string `json:"version"`
	} `json:"loader"`
}

func importInstances(client *rpc.Client, args []string) error {
	var found struct {
		Found []foreignInstance `json:"found"`
	}
	if err := client.Call("import.scan", nil, &found); err != nil {
		return err
	}

	if len(found.Found) == 0 {
		fmt.Println("No Prism, PolyMC or MultiMC instances found.")
		return nil
	}

	wanted := ""
	if len(args) > 0 {
		wanted = args[0]
	}
	renameTo := ""
	if len(args) > 1 {
		renameTo = args[1]
	}

	if wanted == "" {
		writer := table()
		fmt.Fprintln(writer, "INSTANCE\tVERSION\tLOADER\tSTATE")
		for _, one := range found.Found {
			loader := "vanilla"
			if one.Loader != nil {
				loader = one.Loader.Kind + " " + one.Loader.Version
			}
			state := "ready"
			if one.BlockedBy != "" {
				state = one.BlockedBy + " is not supported yet"
			}
			fmt.Fprintf(writer, "%s\t%s\t%s\t%s\n", one.Name, one.Version, loader, state)
		}
		if err := writer.Flush(); err != nil {
			return err
		}
		fmt.Println("\nRun: acelus import <instance>")
		return nil
	}

	for _, one := range found.Found {
		if !strings.EqualFold(one.Name, wanted) {
			continue
		}

		var done struct {
			Instance struct {
				ID   string `json:"id"`
				Path string `json:"path"`
			} `json:"instance"`
			CopiedBytes int64 `json:"copiedBytes"`
		}
		params := map[string]any{"path": one.Path}
		if renameTo != "" {
			params["name"] = renameTo
		}
		if err := client.Call("import.run", params, &done); err != nil {
			return err
		}

		fmt.Printf("Imported %s to %s (%s of your files)\n",
			one.Name, done.Instance.Path, humanBytes(done.CopiedBytes))
		fmt.Printf("Your Prism copy is untouched.\nNext: acelus install %s\n", done.Instance.ID)
		return nil
	}

	names := make([]string, 0, len(found.Found))
	for _, one := range found.Found {
		names = append(names, one.Name)
	}
	return fmt.Errorf("no other launcher has an instance named %q; found: %s",
		wanted, strings.Join(names, ", "))
}

func instances(client *rpc.Client) error {
	var listing struct {
		Instances []struct {
			ID        string `json:"id"`
			Version   string `json:"version"`
			Installed bool   `json:"installed"`
			Path      string `json:"path"`
			Loader    *struct {
				Kind    string `json:"kind"`
				Version string `json:"version"`
			} `json:"loader"`
		} `json:"instances"`
	}
	if err := client.Call("instance.list", nil, &listing); err != nil {
		return err
	}

	if len(listing.Instances) == 0 {
		fmt.Println("No instances yet. Run: acelus create <name> <version>")
		return nil
	}

	writer := table()
	fmt.Fprintln(writer, "INSTANCE\tVERSION\tLOADER\tSTATE")
	for _, instance := range listing.Instances {
		state := "not installed"
		if instance.Installed {
			state = "installed"
		}
		loader := "vanilla"
		if instance.Loader != nil {
			loader = instance.Loader.Kind
			if instance.Loader.Version != "" {
				loader += " " + instance.Loader.Version
			}
		}
		fmt.Fprintf(writer, "%s\t%s\t%s\t%s\n", instance.ID, instance.Version, loader, state)
	}
	return writer.Flush()
}

func install(client *rpc.Client, args []string) error {
	id, err := requireArg(args, 0, "an instance name")
	if err != nil {
		return err
	}

	var done struct {
		Artifacts int   `json:"artifacts"`
		Bytes     int64 `json:"bytes"`
	}

	lastPhase := ""
	err = client.CallWithNotifications("install.run", map[string]any{"id": id}, &done,
		func(method string, params json.RawMessage) {
			if method != "install.progress" {
				return
			}
			var progress struct {
				Phase          string `json:"phase"`
				CompletedFiles int64  `json:"completedFiles"`
				TotalFiles     int64  `json:"totalFiles"`
				ReusedBytes    int64  `json:"reusedBytes"`
			}
			if json.Unmarshal(params, &progress) != nil {
				return
			}
			if progress.Phase != lastPhase {
				lastPhase = progress.Phase
				fmt.Printf("%s...\n", progress.Phase)
			}
		})
	if err != nil {
		return err
	}

	fmt.Printf("Installed %d files (%s)\n", done.Artifacts, humanBytes(done.Bytes))
	fmt.Printf("Next: acelus launch %s\n", id)
	return nil
}

func verify(client *rpc.Client, args []string) error {
	id, err := requireArg(args, 0, "an instance name")
	if err != nil {
		return err
	}

	var report struct {
		OK       bool `json:"ok"`
		Problems []struct {
			Path string `json:"path"`
			Kind string `json:"kind"`
		} `json:"problems"`
	}
	if err := client.Call("verify.run", map[string]any{"id": id}, &report); err != nil {
		return err
	}

	if report.OK {
		fmt.Println("Everything matches the lockfile.")
		return nil
	}

	writer := table()
	fmt.Fprintln(writer, "PROBLEM\tFILE")
	for _, problem := range report.Problems {
		fmt.Fprintf(writer, "%s\t%s\n", problem.Kind, problem.Path)
	}
	if err := writer.Flush(); err != nil {
		return err
	}
	return fmt.Errorf("%d file(s) do not match; run acelus install %s to repair",
		len(report.Problems), id)
}

func launch(client *rpc.Client, args []string) error {
	id, err := requireArg(args, 0, "an instance name")
	if err != nil {
		return err
	}

	var started struct {
		SessionID string `json:"sessionId"`
		PID       int    `json:"pid"`
	}
	if err := client.Call("launch.start", map[string]any{"id": id}, &started); err != nil {
		return err
	}

	fmt.Printf("Started %s (pid %d)\n", started.SessionID, started.PID)
	fmt.Printf("Logs: acelus logs %s\n", started.SessionID)
	return nil
}

func running(client *rpc.Client) error {
	var status struct {
		Sessions []struct {
			SessionID string `json:"sessionId"`
			PID       int    `json:"pid"`
		} `json:"sessions"`
	}
	if err := client.Call("launch.status", nil, &status); err != nil {
		return err
	}

	if len(status.Sessions) == 0 {
		fmt.Println("Nothing running.")
		return nil
	}

	writer := table()
	fmt.Fprintln(writer, "SESSION\tPID")
	for _, session := range status.Sessions {
		fmt.Fprintf(writer, "%s\t%d\n", session.SessionID, session.PID)
	}
	return writer.Flush()
}

func stop(client *rpc.Client, args []string) error {
	id, err := requireArg(args, 0, "a session id")
	if err != nil {
		return err
	}
	return client.Call("launch.stop", map[string]any{"sessionId": id}, nil)
}

func logs(client *rpc.Client, args []string) error {
	id, err := requireArg(args, 0, "a session id")
	if err != nil {
		return err
	}

	var captured struct {
		Lines []struct {
			Stream string `json:"stream"`
			Line   string `json:"line"`
		} `json:"lines"`
	}
	if err := client.Call("log.tail", map[string]any{"sessionId": id}, &captured); err != nil {
		return err
	}

	for _, line := range captured.Lines {
		if line.Stream == "stderr" {
			fmt.Fprintln(os.Stderr, line.Line)
		} else {
			fmt.Println(line.Line)
		}
	}
	return nil
}

func remove(client *rpc.Client, args []string) error {
	id, err := requireArg(args, 0, "an instance name")
	if err != nil {
		return err
	}

	return client.Call("instance.delete", map[string]any{
		"id":           id,
		"keepUserData": contains(args, "--keep-user-data"),
	}, nil)
}

func contains(args []string, flag string) bool {
	for _, arg := range args {
		if arg == flag {
			return true
		}
	}
	return false
}

func loaderSpec(args []string) (map[string]any, error) {
	var chosen map[string]any
	for _, kind := range []string{"fabric", "quilt"} {
		flag := "--" + kind
		version, requested := pinnedFlag(args, flag)
		if !requested {
			continue
		}
		if chosen != nil {
			return nil, fmt.Errorf("an instance takes one mod loader, not both")
		}
		chosen = map[string]any{"kind": kind}
		if version != "" {
			chosen["version"] = version
		}
	}
	return chosen, nil
}

func pinnedFlag(args []string, flag string) (string, bool) {
	for _, arg := range args {
		if arg == flag {
			return "", true
		}
		if after, found := strings.CutPrefix(arg, flag+"="); found {
			return after, true
		}
	}
	return "", false
}

func flagValue(args []string, flag string) (string, bool) {
	for index, arg := range args {
		if arg == flag && index+1 < len(args) {
			return args[index+1], true
		}
		if after, found := strings.CutPrefix(arg, flag+"="); found {
			return after, true
		}
	}
	return "", false
}

func humanBytes(bytes int64) string {
	const unit = 1024
	if bytes < unit {
		return fmt.Sprintf("%d B", bytes)
	}
	div, exp := int64(unit), 0
	for n := bytes / unit; n >= unit; n /= unit {
		div *= unit
		exp++
	}
	return fmt.Sprintf("%.1f %ciB", float64(bytes)/float64(div), "KMGTPE"[exp])
}
