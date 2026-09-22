// inventory: mom_irf_matching_statement
// Original AR(1) plus a definition. `method_of_moments` is a `;` statement whose
// option list is recorded; `mom_method` is the option the check pass reads.
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

method_of_moments(mom_method=IRF_MATCHING);
