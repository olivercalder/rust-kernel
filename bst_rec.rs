struct Node {
    val:    u32,
    left:   Option<Box<Node>>, // In order to allow the possibility of null values,
    right:  Option<Box<Node>>  // wrap such variables in the Option enum.
}

impl Node {
    fn new(newval: u32) -> Box<Node> {
        Box::new(Node {val: newval, left: None, right: None})
    }
    // Returns true if value unique, false if value was already in tree
    fn add(&mut self, newval: u32) -> bool {
        if newval == self.val {
            return false;
        } else if newval < self.val {
            return match &mut self.left {
                None => {
                    self.left = Some(Node::new(newval));
                    true
                },
                Some(n) => n.add(newval)
            }
        } else {
            return match &mut self.right {
                None => {
                    self.right = Some(Node::new(newval));
                    true
                },
                Some(n) => n.add(newval)
            }
        }
    }
    // Returns true if value in tree, else false
    fn contains(&self, val: u32) -> bool {
        if val == self.val { return true }
        else if val < self.val {
            return match &self.left {
                None => false,
                Some(n) => n.contains(val)
            }
        } else {
            return match &self.right {
                None => false,
                Some(n) => n.contains(val)
            }
        }
    }
    fn print(&self) -> () {
        match &self.left {
            None => {},
            Some(n) => n.print()
        }
        println!("{}", self.val);
        match &self.right {
            None => {},
            Some(n) => n.print()
        }
    }
}

fn main() {
    let mut tree = Node::new(6);
    for val in vec![2, 7, 5, 3, 1, 9, 4, 9] {
        tree.add(val);
    }
    tree.print();
    println!("Contains 3? {}", tree.contains(3));
    println!("Contains 8? {}", tree.contains(8));
}
