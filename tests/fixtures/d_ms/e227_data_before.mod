// inventory: e227_data_before
// 7.1 accepts this file: the `data` statement is written before the estimation it silences
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
data(file='x.csv');
estimation;
