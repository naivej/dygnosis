// inventory: lists_e239_rplot_z
// `rplot` names a symbol the file never declares.
var y c;
varexo e;
varexo_det ed;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

rplot z;
