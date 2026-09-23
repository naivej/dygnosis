// inventory: e001_opener_var_decl_only
// The declaration is legal on its own; 7.1 accepts it, and the only complaint is
// that the second endogenous is never used in the model (`W013` / `W020`).
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
