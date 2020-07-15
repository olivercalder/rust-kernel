struct Node {
    val:    u32,
    left:   Option<Node>, // In order to allow the possibility of null values,
    right:  Option<Node>  // wrap such variables in the Option enum.
}

fn new_node(val: u32) {
    Node {val, None, None}
}

impl Node {
    // Returns true if value unique, false if value was already in tree
    fn add(&mut self, newval: u32) -> bool {
        if newval == self.val {
            return false;
        } else if newval < self.val {
            return match self.left {
                None => {
                    self.left = new_node(newval);
                    true
                },
                Some(n) => n.add(newval)
            }
        } else {
            return match self.right {
                None => {
                    self.right = new_node(newval);
                    true
                },
                Some(n) => n.add(newval)
            }
        }
    }
    // Returns true if value in tree, else false
    fn contains(&self, val: u32) -> bool {
        if newval == self.val { return true }
        else if newval < self.val {
            return match self.left {
                None => false;
                Some(n) => self.left.contains(val)
            }
        } else {
            return match self.right {
                None => false;
                Some(n) => self.right.contains(val)
            }
        }
    }
    fn print(&self) -> () {
        match self.left {
            None => {};
            Some(n) => self.left.print()
        }
        println!(self.val);
        match self.right {
            None => {};
            Some(n) => self.right.print()
        }
    }
}

fn main() {
    let mut tree = new_node(6);
    for val in vec![2, 7, 5, 3, 1, 9, 4, 9] {
        tree.add(val);
    }
    tree.print()
}
