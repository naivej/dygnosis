// inventory: e010_osr_gap
var y z;
varexo e;
parameters rho;
rho = 0.5;

model;
y = rho * y(-1) + z + e;
end;

osr(instruments=(y));
