// inventory: lists_e240_realtime_a
// `realtime_shock_decomposition` takes endogenous only; `a` is a parameter.
var y c;
varexo e;
varexo_det ed;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

realtime_shock_decomposition a;
