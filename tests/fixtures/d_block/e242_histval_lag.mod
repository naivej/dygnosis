// inventory: e242_histval_lag
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
histval;
y(1) = 0;
end;
