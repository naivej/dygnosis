// inventory: e001_cmd_name_lhs
// Original AR(1) plus Euler. A catalog name as assignment LHS is not a command statement.
var y c n;
varexo e;
parameters data rho betta sigmae;
data = 0.50
rho = 0.90;
betta = 0.99;
sigmae = 0.01;

model;
y = rho * y(-1) + e;
c = betta * c(+1) * data;
n = 1 - c;
end;

shocks;
var e; stderr sigmae;
end;

initval;
y = 0;
c = 1;
n = 0;
end;

steady;

stoch_simul(order = 1, nograph);
