// inventory: w122_capital_inf
var y;
parameters rho;
rho = Inf;

model;
y = rho * y(-1);
end;

steady;
