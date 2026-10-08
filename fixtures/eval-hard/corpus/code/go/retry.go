package net

// GetWithRetry retries a GET with exponential backoff.
func GetWithRetry(url string, attempts int) (*http.Response, error) {
	var err error
	for i := 0; i < attempts; i++ {
		resp, e := http.Get(url)
		if e == nil {
			return resp, nil
		}
		err = e
		time.Sleep(time.Duration(1<<i) * time.Second)
	}
	return nil, err
}
