/// Least-recently-used cache with a fixed capacity.
pub struct Lru<K, V> {
    capacity: usize,
    map: HashMap<K, V>,
    order: VecDeque<K>,
}
