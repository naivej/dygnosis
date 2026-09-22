// inventory: lists_e240_rplot_ed
// `ed` is a `varexo_det`, a type of its own: not an `exogenous` for `rplot`.
var y c;
varexo e;
varexo_det ed;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

rplot ed;
