// inventory: w140_linear_log
var y;
varexo e;
parameters rho;
rho = 0.9;
model(linear);
y = log(y) + e;
end;
