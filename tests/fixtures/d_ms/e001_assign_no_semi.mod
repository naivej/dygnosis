// inventory: d_ms_e001_assign_no_semi
// A genuine parameter assignment with no terminating `;`. The family spans must
// not swallow this: 7.1 refuses the file and so do we.
var y c k;
varexo e;
parameters alpha beta gamma;
alpha = 0.36
beta = 0.99;
model;
c = alpha*y + beta*c(-1) + e;
y = beta*y(-1) + c;
k = y;
end;
initval;
y = 0;
c = 0;
k = 0;
end;
shocks;
var e; stderr 0.1;
end;
sbvar(freq=4);
