// inventory: lists_e240_icdec_a
// `initial_condition_decomposition` takes endogenous only; `a` is a parameter.
var y c;
varexo e;
varexo_det ed;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

initial_condition_decomposition a;
