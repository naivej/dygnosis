// inventory: e001_opener_var_late_names
// One of the names the first sweep missed: declared and used, which 7.1 accepts.
var y heteroskedastic_shocks;
varexo e;
parameters rho;
rho = 0.95;

model;
y = rho * y(-1) + e + heteroskedastic_shocks;
heteroskedastic_shocks = 0.1 * y;
end;

initval;
y = 0;
heteroskedastic_shocks = 0;
end;

shocks;
var e; stderr 0.01;
end;

stoch_simul(order = 1, nograph);
