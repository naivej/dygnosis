// inventory: companions_dup_path
// Same path from a catalog option and a leftover quote: one record.
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

data(file='dup.csv');
irfs = importdata('dup.csv');
