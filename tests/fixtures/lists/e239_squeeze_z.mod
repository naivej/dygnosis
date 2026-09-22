// inventory: lists_e239_squeeze_z
// `squeeze_shock_decomposition` names a symbol the file never declares.
var y c;
varexo e;
varexo_det ed;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

squeeze_shock_decomposition z;
