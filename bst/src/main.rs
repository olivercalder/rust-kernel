mod bst_rec;
use bst_rec::Node;

fn main() {
    let mut tree = Node::new(6);
    for val in vec![2, 7, 5, 3, 1, 9, 4, 9] {
        tree.add(val);
    }
    tree.print();
    println!("Contains 3? {}", tree.contains(3));
    println!("Contains 8? {}", tree.contains(8));
}
