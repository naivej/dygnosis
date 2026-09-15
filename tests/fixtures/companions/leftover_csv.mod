// inventory: companions_leftover_csv
// Quoted leftover csv is not a catalog option.
var y;
varexo e;
parameters rho;
rho = 0.5;

model;
y = rho * y(-1) + e;
end;

initval;
y = 0;
end;

irfs = importdata('leftover.csv');
