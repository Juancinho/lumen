package cache

// LRU evicts the least recently used entry when full.
type LRU struct {
	cap   int
	order *list.List
	items map[string]*list.Element
}
