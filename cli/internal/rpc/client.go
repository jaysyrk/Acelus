package rpc

import (
	"bufio"
	"encoding/json"
	"errors"
	"fmt"
	"net"
	"os"
	"os/exec"
	"path/filepath"
	"time"
)

const dialAttempts = 40

type Error struct {
	Code    int             `json:"code"`
	Message string          `json:"message"`
	Data    json.RawMessage `json:"data,omitempty"`
}

func (e *Error) Error() string {
	return e.Message
}

type frame struct {
	ID     *json.RawMessage `json:"id"`
	Result json.RawMessage  `json:"result"`
	Error  *Error           `json:"error"`
	Method string           `json:"method"`
	Params json.RawMessage  `json:"params"`
}

type request struct {
	Version string `json:"jsonrpc"`
	ID      int    `json:"id"`
	Method  string `json:"method"`
	Params  any    `json:"params"`
}

type NotificationHandler func(method string, params json.RawMessage)

type Client struct {
	conn   net.Conn
	reader *bufio.Reader
	nextID int
}

func SocketPath() string {
	if explicit := os.Getenv("ACELUS_SOCKET"); explicit != "" {
		return explicit
	}
	if home := os.Getenv("ACELUS_HOME"); home != "" {
		return filepath.Join(home, "acelusd.sock")
	}

	base := os.Getenv("XDG_DATA_HOME")
	if base == "" {
		home, err := os.UserHomeDir()
		if err != nil {
			return "acelusd.sock"
		}
		base = filepath.Join(home, ".local", "share")
	}
	return filepath.Join(base, "acelus", "acelusd.sock")
}

func Dial(socket string) (*Client, error) {
	conn, err := net.Dial("unix", socket)
	if err != nil {
		return nil, err
	}
	return &Client{conn: conn, reader: bufio.NewReader(conn)}, nil
}

func DialOrStart(socket string) (*Client, error) {
	client, err := Dial(socket)
	if err == nil {
		return client, nil
	}

	if startErr := startDaemon(); startErr != nil {
		return nil, fmt.Errorf("acelusd is not running and could not be started: %w", startErr)
	}

	for attempt := 0; attempt < dialAttempts; attempt++ {
		time.Sleep(50 * time.Millisecond)
		if client, err := Dial(socket); err == nil {
			return client, nil
		}
	}

	return nil, errors.New("acelusd did not start listening in time")
}

func startDaemon() error {
	binary := os.Getenv("ACELUSD_BINARY")
	if binary == "" {
		binary = "acelusd"
	}

	command := exec.Command(binary)
	command.Stdout = nil
	command.Stderr = nil
	return command.Start()
}

func (c *Client) Close() error {
	return c.conn.Close()
}

func (c *Client) Call(method string, params any, result any) error {
	return c.CallWithNotifications(method, params, result, nil)
}

func (c *Client) CallWithNotifications(
	method string,
	params any,
	result any,
	onNotify NotificationHandler,
) error {
	c.nextID++
	id := c.nextID

	if params == nil {
		params = map[string]any{}
	}

	payload, err := json.Marshal(request{Version: "2.0", ID: id, Method: method, Params: params})
	if err != nil {
		return err
	}

	if _, err := c.conn.Write(append(payload, '\n')); err != nil {
		return err
	}

	for {
		line, err := c.reader.ReadBytes('\n')
		if err != nil {
			return fmt.Errorf("the connection to acelusd ended: %w", err)
		}

		var received frame
		if err := json.Unmarshal(line, &received); err != nil {
			continue
		}

		if received.ID == nil {
			if onNotify != nil && received.Method != "" {
				onNotify(received.Method, received.Params)
			}
			continue
		}

		var replyID int
		if err := json.Unmarshal(*received.ID, &replyID); err != nil || replyID != id {
			continue
		}

		if received.Error != nil {
			return received.Error
		}

		if result != nil && len(received.Result) > 0 {
			return json.Unmarshal(received.Result, result)
		}
		return nil
	}
}

func (c *Client) Await(onNotify NotificationHandler, until func(method string) bool) error {
	for {
		line, err := c.reader.ReadBytes('\n')
		if err != nil {
			return fmt.Errorf("the connection to acelusd ended: %w", err)
		}

		var received frame
		if err := json.Unmarshal(line, &received); err != nil {
			continue
		}
		if received.Method == "" {
			continue
		}

		if onNotify != nil {
			onNotify(received.Method, received.Params)
		}
		if until != nil && until(received.Method) {
			return nil
		}
	}
}
