// inventory: mom_matched_irfs_rows
// Original AR(1) plus a definition. One `matched_irfs` row: an endogenous, a
// shock, two period entries and two values, with no weights keyword.
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

matched_irfs;
var y; varexo e; periods 1 2; values 1 2; end;
