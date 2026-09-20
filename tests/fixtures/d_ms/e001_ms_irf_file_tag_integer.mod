// inventory: e001_ms_irf_file_tag_integer
// `ms_irf(file_tag=1) y;` — `file_tag` takes a `filename`; 7.1 refuses `unexpected INT_NUMBER`.
var y c k R Pie Y;
varexo e eps;
parameters alpha beta gamma;
alpha = 0.36;
beta = 0.99;
gamma = 0.5;
model;
c = alpha*y + beta*c(-1) + e;
y = beta*y(-1) + c;
k = y;
R = beta*R(-1) + eps;
Pie = alpha*R(-1) + eps;
Y = beta*Pie(-1) + eps;
end;
initval;
y = 0;
c = 0;
k = 0;
R = 0;
Pie = 0;
Y = 0;
end;
shocks;
var e; stderr 0.1;
var eps; stderr 0.1;
end;
ms_irf(file_tag=1) y;
