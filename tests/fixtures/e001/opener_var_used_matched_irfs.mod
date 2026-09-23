// inventory: e001_opener_var_used_matched_irfs
// The shape the finding came from: `matched_irfs` is declared as an endogenous and
// used in the model, which 7.1 accepts.
var y matched_irfs;
varexo e;
parameters rho;
rho = 0.95;

model;
y = rho * y(-1) + e + matched_irfs;
matched_irfs = 0.1 * y;
end;

initval;
y = 0;
matched_irfs = 0;
end;

shocks;
var e; stderr 0.01;
end;

stoch_simul(order = 1, nograph);
