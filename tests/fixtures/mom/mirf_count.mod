// inventory: mom_e390_periods_values_count
// `periods 1:2` is one entry while `values (1) (2)` is two, so the two lists do
// not match.
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

matched_irfs;
var y; varexo e; periods 1:2; values (1) (2);
end;
