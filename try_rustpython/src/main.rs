use try_rustpython::rpy;

fn main() {
    rpy::hello();
    rpy::exec_str(r#"
import sys
# cannot import os
print([i**2 for i in range(10)])
print('printing to stderr', file=sys.stderr)
for item in range(10):
    print(item)
"#).expect("something happened");
}
