// inventory: e340_data_nobs_zero
// 7.1 refuses this file: `The nobs option of the data statement only accepts positive integers.`
var y c;
varexo e;
parameters alpha beta;
alpha = 0.5;
beta = 0.9;
model;
y = alpha*c + beta*y(-1) + e;
c = y(-1) + beta*c(-1) + e;
end;
initval;
y = 0;
c = 0;
end;
shocks;
var e; stderr 0.1;
end;
varobs y;
data(file='x.csv', nobs=0);
