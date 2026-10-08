package fetch

import (
	"net/http"
	"time"
)

// NewClient returns an HTTP client with sane timeouts for calling internal services.
func NewClient() *http.Client {
	return &http.Client{
		Timeout: 15 * time.Second,
		Transport: &http.Transport{MaxIdleConnsPerHost: 16, IdleConnTimeout: 90 * time.Second},
	}
}
