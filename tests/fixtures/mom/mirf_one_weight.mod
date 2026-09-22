// inventory: mom_quiet_irf_one_weight
// `irf_matching` in lowercase with one weight on two periods: 7.1 broadcasts the
// single weight, so the counts do not have to match.
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

method_of_moments(mom_method = irf_matching);
matched_irfs;
var y; varexo e; periods 1 2; values 1 2; weights 3;
end;
