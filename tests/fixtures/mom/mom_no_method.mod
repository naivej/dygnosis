// inventory: mom_e382_no_method
// `method_of_moments` with a non-empty option list but no `mom_method`. 7.1's
// checkPass refuses: the method must be named.
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

method_of_moments(order = 1);
