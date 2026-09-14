// inventory: e001_var_cmd_name
// Original AR(1) plus Euler. Catalog command names stay legal identifiers (0.1 reserved list).
var method_of_moments y c;
varexo e;
parameters rho betta sigmae;
rho = 0.90;
betta = 0.99;
sigmae = 0.01;

model;
method_of_moments = rho * method_of_moments(-1) + e;
y = betta * method_of_moments(+1);
c = y - method_of_moments;
end;

shocks;
var e; stderr sigmae;
end;

initval;
method_of_moments = 0;
y = 0;
c = 0;
end;

steady;

stoch_simul(order = 1, nograph);
