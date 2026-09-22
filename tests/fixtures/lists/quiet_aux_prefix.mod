// inventory: lists_quiet_aux_prefix
// Undeclared `AUX_ENDO_1` is passed over by their regex, and the hit stops the
// statement list, so the second name is never read.
var y c;
varexo e;
varexo_det ed;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

forecast AUX_ENDO_1, z;
stoch_simul AUX_ENDO_1, a;
