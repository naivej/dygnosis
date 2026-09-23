// inventory: pin72_quiet_initval_file_last_simulation_period
var y;
varexo e;
parameters rho;
rho = .5;
model;
y = rho*y(-1) + e;
end;
initval_file(last_simulation_period=1);
