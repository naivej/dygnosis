var y (long_name='output');
parameters beta (long_name='discount');
model;
@#include "child.mod"
[name='root'] y = beta*y(-1);
end;
