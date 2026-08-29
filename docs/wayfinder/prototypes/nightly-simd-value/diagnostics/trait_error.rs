fn needs_u32_items(iterator: impl Iterator<Item = u32>) {
    let _ = iterator.count();
}

fn main() {
    needs_u32_items([1_u16, 2, 3].into_iter());
}
