// inventory: lists_quiet_rplot_exo
// `rplot` is the one command whose list takes a plain `varexo`: accepted.
var y c;
varexo e;
varexo_det ed;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

rplot e;
