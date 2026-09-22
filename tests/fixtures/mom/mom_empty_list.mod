// inventory: mom_dmom_empty_list
// The empty option list on `method_of_moments`: the grammar has no production
// for it, so 7.1 refuses it while parsing (`syntax error, unexpected ')'`).
// P-mom handed the shape over; this slice reports it as E001 with a hint.
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

method_of_moments();
