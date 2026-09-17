// inventory: e207_no_static
var y;
varexo e;
parameters rho;
rho = 0.9;
model(no_static);
y = rho * y(-1) + e;
end;
check;
