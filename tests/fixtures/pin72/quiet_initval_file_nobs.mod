// inventory: pin72_quiet_initval_file_nobs
var y;
varexo e;
parameters rho;
rho = .5;
model;
y = rho*y(-1) + e;
end;
initval_file(nobs=1);
