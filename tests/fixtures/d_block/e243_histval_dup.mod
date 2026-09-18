// inventory: e243_histval_dup
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
histval;
y(0) = 0;
y(0) = 1;
end;
