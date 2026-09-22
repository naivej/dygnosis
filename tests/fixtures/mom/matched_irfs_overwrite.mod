// inventory: mom_matched_irfs_overwrite
// Original AR(1) plus a definition. The `(overwrite)` list sets the flag; a range
// in `periods`, a parenthesised value and one weight are each one entry.
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

matched_irfs(overwrite);
var y; varexo e; periods 1:2; values (1); weights 3; end;
