// inventory: e001_opener_var_decl_only
// The declaration is legal syntax; Transform later refuses the unused
// endogenous name (E186), before the count check.
var y shocks;
varexo e;
parameters rho;
rho = 0.95;

model;
y = rho * y(-1) + e;
end;

shocks;
var e; stderr 0.01;
end;

stoch_simul(order = 1, nograph);
