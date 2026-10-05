// Pinned manual endval example, with its parameter declarations/calibration.
var c k;
varexo x;
parameters aa alph bet delt gam;
aa=1; alph=0.33; bet=0.01; delt=0.025; gam=1;
model;
c + k - aa*x*k(-1)^alph - (1-delt)*k(-1);
c^(-gam) - (1+bet)^(-1)*(aa*alph*x(+1)*k^(alph-1) + 1 - delt)*c(+1)^(-gam);
end;
initval;
c=1.2; k=12; x=1;
end;
steady;
endval;
c=2; k=20; x=2;
end;
steady;
perfect_foresight_setup(periods=200);
perfect_foresight_solver;
