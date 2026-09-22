// inventory: lists_e240_plotsd_a
// `plot_shock_decomposition` is the one command with an `epilogue` arm; a
// parameter is in neither arm.
var y c;
varexo e;
varexo_det ed;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

plot_shock_decomposition a;
