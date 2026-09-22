// inventory: lists_e240_removed_stoch
// A `model_remove` dropped `c` from the endogenous; 7.1 keeps the name as an
// `excludedVariable`, so the list reports its type.
var y c;
varexo e;
varexo_det ed;
parameters a;
a = 0.5;

model;
[name='e1'] y = a*y(-1) + e;
[name='e2'] c = y;
end;

model_remove([name='e2']);

stoch_simul c;
