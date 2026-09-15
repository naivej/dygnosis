// inventory: companions_data_file
// data(file=) names a sibling csv that must resolve.
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

data(file='data_file.csv');
