/* Real FFTPACK plan, extracted from NumPy v1.26.4 numpy/fft/_pocketfft.c.
 * Source SHA256 5261950f4b8fdf0f12071178abbbb00ffeddc4dfec002446638f248b51faa63a.
 * This standalone subset omits Python, complex FFT and Bluestein entry points.
 * See LICENSE-pocketfft.md. */
/*
 * This file is part of pocketfft.
 * Licensed under a 3-clause BSD style license - see LICENSE.md
 */

/*
 *  Main implementation file.
 *
 *  Copyright (C) 2004-2018 Max-Planck-Society
 *  \author Martin Reinecke
 */
#include <assert.h>
#include <math.h>
#include <string.h>
#include <stdlib.h>

#define RALLOC(type,num) \
  (assert(num != 0), ((type *)malloc((num)*sizeof(type))))
#define DEALLOC(ptr) \
  do { free(ptr); (ptr)=NULL; } while(0)

#define SWAP(a,b,type) \
  do { type tmp_=(a); (a)=(b); (b)=tmp_; } while(0)

#ifdef __GNUC__
#define NOINLINE __attribute__((noinline))
#define WARN_UNUSED_RESULT __attribute__ ((warn_unused_result))
#else
#define NOINLINE
#define WARN_UNUSED_RESULT
#endif

// adapted from https://stackoverflow.com/questions/42792939/
// CAUTION: this function only works for arguments in the range [-0.25; 0.25]!
static void my_sincosm1pi (double a, double *restrict res)
  {
  double s = a * a;
  /* Approximate cos(pi*x)-1 for x in [-0.25,0.25] */
  double r =     -1.0369917389758117e-4;
  r = fma (r, s,  1.9294935641298806e-3);
  r = fma (r, s, -2.5806887942825395e-2);
  r = fma (r, s,  2.3533063028328211e-1);
  r = fma (r, s, -1.3352627688538006e+0);
  r = fma (r, s,  4.0587121264167623e+0);
  r = fma (r, s, -4.9348022005446790e+0);
  double c = r*s;
  /* Approximate sin(pi*x) for x in [-0.25,0.25] */
  r =             4.6151442520157035e-4;
  r = fma (r, s, -7.3700183130883555e-3);
  r = fma (r, s,  8.2145868949323936e-2);
  r = fma (r, s, -5.9926452893214921e-1);
  r = fma (r, s,  2.5501640398732688e+0);
  r = fma (r, s, -5.1677127800499516e+0);
  s = s * a;
  r = r * s;
  s = fma (a, 3.1415926535897931e+0, r);
  res[0] = c;
  res[1] = s;
  }

NOINLINE static void calc_first_octant(size_t den, double * restrict res)
  {
  size_t n = (den+4)>>3;
  if (n==0) return;
  res[0]=1.; res[1]=0.;
  if (n==1) return;
  size_t l1=(size_t)sqrt(n);
  for (size_t i=1; i<l1; ++i)
    my_sincosm1pi((2.*i)/den,&res[2*i]);
  size_t start=l1;
  while(start<n)
    {
    double cs[2];
    my_sincosm1pi((2.*start)/den,cs);
    res[2*start] = cs[0]+1.;
    res[2*start+1] = cs[1];
    size_t end = l1;
    if (start+end>n) end = n-start;
    for (size_t i=1; i<end; ++i)
      {
      double csx[2]={res[2*i], res[2*i+1]};
      res[2*(start+i)] = ((cs[0]*csx[0] - cs[1]*csx[1] + cs[0]) + csx[0]) + 1.;
      res[2*(start+i)+1] = (cs[0]*csx[1] + cs[1]*csx[0]) + cs[1] + csx[1];
      }
    start += l1;
    }
  for (size_t i=1; i<l1; ++i)
    res[2*i] += 1.;
  }

NOINLINE static void calc_first_quadrant(size_t n, double * restrict res)
  {
  double * restrict p = res+n;
  calc_first_octant(n<<1, p);
  size_t ndone=(n+2)>>2;
  size_t i=0, idx1=0, idx2=2*ndone-2;
  for (; i+1<ndone; i+=2, idx1+=2, idx2-=2)
    {
    res[idx1]   = p[2*i];
    res[idx1+1] = p[2*i+1];
    res[idx2]   = p[2*i+3];
    res[idx2+1] = p[2*i+2];
    }
  if (i!=ndone)
    {
    res[idx1  ] = p[2*i];
    res[idx1+1] = p[2*i+1];
    }
  }

NOINLINE static void calc_first_half(size_t n, double * restrict res)
  {
  int ndone=(n+1)>>1;
  double * p = res+n-1;
  calc_first_octant(n<<2, p);
  int i4=0, in=n, i=0;
  for (; i4<=in-i4; ++i, i4+=4) // octant 0
    {
    res[2*i] = p[2*i4]; res[2*i+1] = p[2*i4+1];
    }
  for (; i4-in <= 0; ++i, i4+=4) // octant 1
    {
    int xm = in-i4;
    res[2*i] = p[2*xm+1]; res[2*i+1] = p[2*xm];
    }
  for (; i4<=3*in-i4; ++i, i4+=4) // octant 2
    {
    int xm = i4-in;
    res[2*i] = -p[2*xm+1]; res[2*i+1] = p[2*xm];
    }
  for (; i<ndone; ++i, i4+=4) // octant 3
    {
    int xm = 2*in-i4;
    res[2*i] = -p[2*xm]; res[2*i+1] = p[2*xm+1];
    }
  }

NOINLINE static void fill_first_quadrant(size_t n, double * restrict res)
  {
  const double hsqt2 = 0.707106781186547524400844362104849;
  size_t quart = n>>2;
  if ((n&7)==0)
    res[quart] = res[quart+1] = hsqt2;
  for (size_t i=2, j=2*quart-2; i<quart; i+=2, j-=2)
    {
    res[j  ] = res[i+1];
    res[j+1] = res[i  ];
    }
  }

NOINLINE static void fill_first_half(size_t n, double * restrict res)
  {
  size_t half = n>>1;
  if ((n&3)==0)
    for (size_t i=0; i<half; i+=2)
      {
      res[i+half]   = -res[i+1];
      res[i+half+1] =  res[i  ];
      }
  else
    for (size_t i=2, j=2*half-2; i<half; i+=2, j-=2)
      {
      res[j  ] = -res[i  ];
      res[j+1] =  res[i+1];
      }
  }

NOINLINE static void fill_second_half(size_t n, double * restrict res)
  {
  if ((n&1)==0)
    for (size_t i=0; i<n; ++i)
      res[i+n] = -res[i];
  else
    for (size_t i=2, j=2*n-2; i<n; i+=2, j-=2)
      {
      res[j  ] =  res[i  ];
      res[j+1] = -res[i+1];
      }
  }

NOINLINE static void sincos_2pibyn_half(size_t n, double * restrict res)
  {
  if ((n&3)==0)
    {
    calc_first_octant(n, res);
    fill_first_quadrant(n, res);
    fill_first_half(n, res);
    }
  else if ((n&1)==0)
    {
    calc_first_quadrant(n, res);
    fill_first_half(n, res);
    }
  else
    calc_first_half(n, res);
  }
#define NFCT 25
typedef struct rfftp_fctdata
  {
  size_t fct;
  double *tw, *tws;
  } rfftp_fctdata;

typedef struct rfftp_plan_i
  {
  size_t length, nfct;
  double *mem;
  rfftp_fctdata fct[NFCT];
  } rfftp_plan_i;
typedef struct rfftp_plan_i * rfftp_plan;

#define WA(x,i) wa[(i)+(x)*(ido-1)]
#define PM(a,b,c,d) { a=c+d; b=c-d; }
/* (a+ib) = conj(c+id) * (e+if) */
#define MULPM(a,b,c,d,e,f) { a=c*e+d*f; b=c*f-d*e; }

#define CC(a,b,c) cc[(a)+ido*((b)+l1*(c))]
#define CH(a,b,c) ch[(a)+ido*((b)+cdim*(c))]

NOINLINE static void radf2 (size_t ido, size_t l1, const double * restrict cc,
  double * restrict ch, const double * restrict wa)
  {
  const size_t cdim=2;

  for (size_t k=0; k<l1; k++)
    PM (CH(0,0,k),CH(ido-1,1,k),CC(0,k,0),CC(0,k,1))
  if ((ido&1)==0)
    for (size_t k=0; k<l1; k++)
      {
      CH(    0,1,k) = -CC(ido-1,k,1);
      CH(ido-1,0,k) =  CC(ido-1,k,0);
      }
  if (ido<=2) return;
  for (size_t k=0; k<l1; k++)
    for (size_t i=2; i<ido; i+=2)
      {
      size_t ic=ido-i;
      double tr2, ti2;
      MULPM (tr2,ti2,WA(0,i-2),WA(0,i-1),CC(i-1,k,1),CC(i,k,1))
      PM (CH(i-1,0,k),CH(ic-1,1,k),CC(i-1,k,0),tr2)
      PM (CH(i  ,0,k),CH(ic  ,1,k),ti2,CC(i  ,k,0))
      }
  }

NOINLINE static void radf3(size_t ido, size_t l1, const double * restrict cc,
  double * restrict ch, const double * restrict wa)
  {
  const size_t cdim=3;
  static const double taur=-0.5, taui=0.86602540378443864676;

  for (size_t k=0; k<l1; k++)
    {
    double cr2=CC(0,k,1)+CC(0,k,2);
    CH(0,0,k) = CC(0,k,0)+cr2;
    CH(0,2,k) = taui*(CC(0,k,2)-CC(0,k,1));
    CH(ido-1,1,k) = CC(0,k,0)+taur*cr2;
    }
  if (ido==1) return;
  for (size_t k=0; k<l1; k++)
    for (size_t i=2; i<ido; i+=2)
      {
      size_t ic=ido-i;
      double di2, di3, dr2, dr3;
      MULPM (dr2,di2,WA(0,i-2),WA(0,i-1),CC(i-1,k,1),CC(i,k,1)) // d2=conj(WA0)*CC1
      MULPM (dr3,di3,WA(1,i-2),WA(1,i-1),CC(i-1,k,2),CC(i,k,2)) // d3=conj(WA1)*CC2
      double cr2=dr2+dr3; // c add
      double ci2=di2+di3;
      CH(i-1,0,k) = CC(i-1,k,0)+cr2; // c add
      CH(i  ,0,k) = CC(i  ,k,0)+ci2;
      double tr2 = CC(i-1,k,0)+taur*cr2; // c add
      double ti2 = CC(i  ,k,0)+taur*ci2;
      double tr3 = taui*(di2-di3);  // t3 = taui*i*(d3-d2)?
      double ti3 = taui*(dr3-dr2);
      PM(CH(i-1,2,k),CH(ic-1,1,k),tr2,tr3) // PM(i) = t2+t3
      PM(CH(i  ,2,k),CH(ic  ,1,k),ti3,ti2) // PM(ic) = conj(t2-t3)
      }
  }

NOINLINE static void radf4(size_t ido, size_t l1, const double * restrict cc,
  double * restrict ch, const double * restrict wa)
  {
  const size_t cdim=4;
  static const double hsqt2=0.70710678118654752440;

  for (size_t k=0; k<l1; k++)
    {
    double tr1,tr2;
    PM (tr1,CH(0,2,k),CC(0,k,3),CC(0,k,1))
    PM (tr2,CH(ido-1,1,k),CC(0,k,0),CC(0,k,2))
    PM (CH(0,0,k),CH(ido-1,3,k),tr2,tr1)
    }
  if ((ido&1)==0)
    for (size_t k=0; k<l1; k++)
      {
      double ti1=-hsqt2*(CC(ido-1,k,1)+CC(ido-1,k,3));
      double tr1= hsqt2*(CC(ido-1,k,1)-CC(ido-1,k,3));
      PM (CH(ido-1,0,k),CH(ido-1,2,k),CC(ido-1,k,0),tr1)
      PM (CH(    0,3,k),CH(    0,1,k),ti1,CC(ido-1,k,2))
      }
  if (ido<=2) return;
  for (size_t k=0; k<l1; k++)
    for (size_t i=2; i<ido; i+=2)
      {
      size_t ic=ido-i;
      double ci2, ci3, ci4, cr2, cr3, cr4, ti1, ti2, ti3, ti4, tr1, tr2, tr3, tr4;
      MULPM(cr2,ci2,WA(0,i-2),WA(0,i-1),CC(i-1,k,1),CC(i,k,1))
      MULPM(cr3,ci3,WA(1,i-2),WA(1,i-1),CC(i-1,k,2),CC(i,k,2))
      MULPM(cr4,ci4,WA(2,i-2),WA(2,i-1),CC(i-1,k,3),CC(i,k,3))
      PM(tr1,tr4,cr4,cr2)
      PM(ti1,ti4,ci2,ci4)
      PM(tr2,tr3,CC(i-1,k,0),cr3)
      PM(ti2,ti3,CC(i  ,k,0),ci3)
      PM(CH(i-1,0,k),CH(ic-1,3,k),tr2,tr1)
      PM(CH(i  ,0,k),CH(ic  ,3,k),ti1,ti2)
      PM(CH(i-1,2,k),CH(ic-1,1,k),tr3,ti4)
      PM(CH(i  ,2,k),CH(ic  ,1,k),tr4,ti3)
      }
  }

NOINLINE static void radf5(size_t ido, size_t l1, const double * restrict cc,
  double * restrict ch, const double * restrict wa)
  {
  const size_t cdim=5;
  static const double tr11= 0.3090169943749474241, ti11=0.95105651629515357212,
                      tr12=-0.8090169943749474241, ti12=0.58778525229247312917;

  for (size_t k=0; k<l1; k++)
    {
    double cr2, cr3, ci4, ci5;
    PM (cr2,ci5,CC(0,k,4),CC(0,k,1))
    PM (cr3,ci4,CC(0,k,3),CC(0,k,2))
    CH(0,0,k)=CC(0,k,0)+cr2+cr3;
    CH(ido-1,1,k)=CC(0,k,0)+tr11*cr2+tr12*cr3;
    CH(0,2,k)=ti11*ci5+ti12*ci4;
    CH(ido-1,3,k)=CC(0,k,0)+tr12*cr2+tr11*cr3;
    CH(0,4,k)=ti12*ci5-ti11*ci4;
    }
  if (ido==1) return;
  for (size_t k=0; k<l1;++k)
    for (size_t i=2; i<ido; i+=2)
      {
      double ci2, di2, ci4, ci5, di3, di4, di5, ci3, cr2, cr3, dr2, dr3,
         dr4, dr5, cr5, cr4, ti2, ti3, ti5, ti4, tr2, tr3, tr4, tr5;
      size_t ic=ido-i;
      MULPM (dr2,di2,WA(0,i-2),WA(0,i-1),CC(i-1,k,1),CC(i,k,1))
      MULPM (dr3,di3,WA(1,i-2),WA(1,i-1),CC(i-1,k,2),CC(i,k,2))
      MULPM (dr4,di4,WA(2,i-2),WA(2,i-1),CC(i-1,k,3),CC(i,k,3))
      MULPM (dr5,di5,WA(3,i-2),WA(3,i-1),CC(i-1,k,4),CC(i,k,4))
      PM(cr2,ci5,dr5,dr2)
      PM(ci2,cr5,di2,di5)
      PM(cr3,ci4,dr4,dr3)
      PM(ci3,cr4,di3,di4)
      CH(i-1,0,k)=CC(i-1,k,0)+cr2+cr3;
      CH(i  ,0,k)=CC(i  ,k,0)+ci2+ci3;
      tr2=CC(i-1,k,0)+tr11*cr2+tr12*cr3;
      ti2=CC(i  ,k,0)+tr11*ci2+tr12*ci3;
      tr3=CC(i-1,k,0)+tr12*cr2+tr11*cr3;
      ti3=CC(i  ,k,0)+tr12*ci2+tr11*ci3;
      MULPM(tr5,tr4,cr5,cr4,ti11,ti12)
      MULPM(ti5,ti4,ci5,ci4,ti11,ti12)
      PM(CH(i-1,2,k),CH(ic-1,1,k),tr2,tr5)
      PM(CH(i  ,2,k),CH(ic  ,1,k),ti5,ti2)
      PM(CH(i-1,4,k),CH(ic-1,3,k),tr3,tr4)
      PM(CH(i  ,4,k),CH(ic  ,3,k),ti4,ti3)
      }
  }

#undef CC
#undef CH
#define C1(a,b,c) cc[(a)+ido*((b)+l1*(c))]
#define C2(a,b) cc[(a)+idl1*(b)]
#define CH2(a,b) ch[(a)+idl1*(b)]
#define CC(a,b,c) cc[(a)+ido*((b)+cdim*(c))]
#define CH(a,b,c) ch[(a)+ido*((b)+l1*(c))]
NOINLINE static void radfg(size_t ido, size_t ip, size_t l1,
  double * restrict cc, double * restrict ch, const double * restrict wa,
  const double * restrict csarr)
  {
  const size_t cdim=ip;
  size_t ipph=(ip+1)/2;
  size_t idl1 = ido*l1;

  if (ido>1)
    {
    for (size_t j=1, jc=ip-1; j<ipph; ++j,--jc)              // 114
      {
      size_t is=(j-1)*(ido-1),
             is2=(jc-1)*(ido-1);
      for (size_t k=0; k<l1; ++k)                            // 113
        {
        size_t idij=is;
        size_t idij2=is2;
        for (size_t i=1; i<=ido-2; i+=2)                      // 112
          {
          double t1=C1(i,k,j ), t2=C1(i+1,k,j ),
                 t3=C1(i,k,jc), t4=C1(i+1,k,jc);
          double x1=wa[idij]*t1 + wa[idij+1]*t2,
                 x2=wa[idij]*t2 - wa[idij+1]*t1,
                 x3=wa[idij2]*t3 + wa[idij2+1]*t4,
                 x4=wa[idij2]*t4 - wa[idij2+1]*t3;
          C1(i  ,k,j ) = x1+x3;
          C1(i  ,k,jc) = x2-x4;
          C1(i+1,k,j ) = x2+x4;
          C1(i+1,k,jc) = x3-x1;
          idij+=2;
          idij2+=2;
          }
        }
      }
    }

  for (size_t j=1, jc=ip-1; j<ipph; ++j,--jc)                // 123
    for (size_t k=0; k<l1; ++k)                              // 122
      {
      double t1=C1(0,k,j), t2=C1(0,k,jc);
      C1(0,k,j ) = t1+t2;
      C1(0,k,jc) = t2-t1;
      }

//everything in C
//memset(ch,0,ip*l1*ido*sizeof(double));

  for (size_t l=1,lc=ip-1; l<ipph; ++l,--lc)                 // 127
    {
    for (size_t ik=0; ik<idl1; ++ik)                         // 124
      {
      CH2(ik,l ) = C2(ik,0)+csarr[2*l]*C2(ik,1)+csarr[4*l]*C2(ik,2);
      CH2(ik,lc) = csarr[2*l+1]*C2(ik,ip-1)+csarr[4*l+1]*C2(ik,ip-2);
      }
    size_t iang = 2*l;
    size_t j=3, jc=ip-3;
    for (; j<ipph-3; j+=4,jc-=4)              // 126
      {
      iang+=l; if (iang>=ip) iang-=ip;
      double ar1=csarr[2*iang], ai1=csarr[2*iang+1];
      iang+=l; if (iang>=ip) iang-=ip;
      double ar2=csarr[2*iang], ai2=csarr[2*iang+1];
      iang+=l; if (iang>=ip) iang-=ip;
      double ar3=csarr[2*iang], ai3=csarr[2*iang+1];
      iang+=l; if (iang>=ip) iang-=ip;
      double ar4=csarr[2*iang], ai4=csarr[2*iang+1];
      for (size_t ik=0; ik<idl1; ++ik)                       // 125
        {
        CH2(ik,l ) += ar1*C2(ik,j )+ar2*C2(ik,j +1)
                     +ar3*C2(ik,j +2)+ar4*C2(ik,j +3);
        CH2(ik,lc) += ai1*C2(ik,jc)+ai2*C2(ik,jc-1)
                     +ai3*C2(ik,jc-2)+ai4*C2(ik,jc-3);
        }
      }
    for (; j<ipph-1; j+=2,jc-=2)              // 126
      {
      iang+=l; if (iang>=ip) iang-=ip;
      double ar1=csarr[2*iang], ai1=csarr[2*iang+1];
      iang+=l; if (iang>=ip) iang-=ip;
      double ar2=csarr[2*iang], ai2=csarr[2*iang+1];
      for (size_t ik=0; ik<idl1; ++ik)                       // 125
        {
        CH2(ik,l ) += ar1*C2(ik,j )+ar2*C2(ik,j +1);
        CH2(ik,lc) += ai1*C2(ik,jc)+ai2*C2(ik,jc-1);
        }
      }
    for (; j<ipph; ++j,--jc)              // 126
      {
      iang+=l; if (iang>=ip) iang-=ip;
      double ar=csarr[2*iang], ai=csarr[2*iang+1];
      for (size_t ik=0; ik<idl1; ++ik)                       // 125
        {
        CH2(ik,l ) += ar*C2(ik,j );
        CH2(ik,lc) += ai*C2(ik,jc);
        }
      }
    }
  for (size_t ik=0; ik<idl1; ++ik)                         // 101
    CH2(ik,0) = C2(ik,0);
  for (size_t j=1; j<ipph; ++j)                              // 129
    for (size_t ik=0; ik<idl1; ++ik)                         // 128
      CH2(ik,0) += C2(ik,j);

// everything in CH at this point!
//memset(cc,0,ip*l1*ido*sizeof(double));

  for (size_t k=0; k<l1; ++k)                                // 131
    for (size_t i=0; i<ido; ++i)                             // 130
      CC(i,0,k) = CH(i,k,0);

  for (size_t j=1, jc=ip-1; j<ipph; ++j,--jc)                // 137
    {
    size_t j2=2*j-1;
    for (size_t k=0; k<l1; ++k)                              // 136
      {
      CC(ido-1,j2,k) = CH(0,k,j);
      CC(0,j2+1,k) = CH(0,k,jc);
      }
    }

  if (ido==1) return;

  for (size_t j=1, jc=ip-1; j<ipph; ++j,--jc)                // 140
    {
    size_t j2=2*j-1;
    for(size_t k=0; k<l1; ++k)                               // 139
      for(size_t i=1, ic=ido-i-2; i<=ido-2; i+=2, ic-=2)      // 138
        {
        CC(i   ,j2+1,k) = CH(i  ,k,j )+CH(i  ,k,jc);
        CC(ic  ,j2  ,k) = CH(i  ,k,j )-CH(i  ,k,jc);
        CC(i+1 ,j2+1,k) = CH(i+1,k,j )+CH(i+1,k,jc);
        CC(ic+1,j2  ,k) = CH(i+1,k,jc)-CH(i+1,k,j );
        }
    }
  }
#undef C1
#undef C2
#undef CH2

#undef CH
#undef CC
#define CH(a,b,c) ch[(a)+ido*((b)+l1*(c))]
#define CC(a,b,c) cc[(a)+ido*((b)+cdim*(c))]

NOINLINE static void radb2(size_t ido, size_t l1, const double * restrict cc,
  double * restrict ch, const double * restrict wa)
  {
  const size_t cdim=2;

  for (size_t k=0; k<l1; k++)
    PM (CH(0,k,0),CH(0,k,1),CC(0,0,k),CC(ido-1,1,k))
  if ((ido&1)==0)
    for (size_t k=0; k<l1; k++)
      {
      CH(ido-1,k,0) = 2.*CC(ido-1,0,k);
      CH(ido-1,k,1) =-2.*CC(0    ,1,k);
      }
  if (ido<=2) return;
  for (size_t k=0; k<l1;++k)
    for (size_t i=2; i<ido; i+=2)
      {
      size_t ic=ido-i;
      double ti2, tr2;
      PM (CH(i-1,k,0),tr2,CC(i-1,0,k),CC(ic-1,1,k))
      PM (ti2,CH(i  ,k,0),CC(i  ,0,k),CC(ic  ,1,k))
      MULPM (CH(i,k,1),CH(i-1,k,1),WA(0,i-2),WA(0,i-1),ti2,tr2)
      }
  }

NOINLINE static void radb3(size_t ido, size_t l1, const double * restrict cc,
  double * restrict ch, const double * restrict wa)
  {
  const size_t cdim=3;
  static const double taur=-0.5, taui=0.86602540378443864676;

  for (size_t k=0; k<l1; k++)
    {
    double tr2=2.*CC(ido-1,1,k);
    double cr2=CC(0,0,k)+taur*tr2;
    CH(0,k,0)=CC(0,0,k)+tr2;
    double ci3=2.*taui*CC(0,2,k);
    PM (CH(0,k,2),CH(0,k,1),cr2,ci3);
    }
  if (ido==1) return;
  for (size_t k=0; k<l1; k++)
    for (size_t i=2; i<ido; i+=2)
      {
      size_t ic=ido-i;
      double tr2=CC(i-1,2,k)+CC(ic-1,1,k); // t2=CC(I) + conj(CC(ic))
      double ti2=CC(i  ,2,k)-CC(ic  ,1,k);
      double cr2=CC(i-1,0,k)+taur*tr2;     // c2=CC +taur*t2
      double ci2=CC(i  ,0,k)+taur*ti2;
      CH(i-1,k,0)=CC(i-1,0,k)+tr2;         // CH=CC+t2
      CH(i  ,k,0)=CC(i  ,0,k)+ti2;
      double cr3=taui*(CC(i-1,2,k)-CC(ic-1,1,k));// c3=taui*(CC(i)-conj(CC(ic)))
      double ci3=taui*(CC(i  ,2,k)+CC(ic  ,1,k));
      double di2, di3, dr2, dr3;
      PM(dr3,dr2,cr2,ci3) // d2= (cr2-ci3, ci2+cr3) = c2+i*c3
      PM(di2,di3,ci2,cr3) // d3= (cr2+ci3, ci2-cr3) = c2-i*c3
      MULPM(CH(i,k,1),CH(i-1,k,1),WA(0,i-2),WA(0,i-1),di2,dr2) // ch = WA*d2
      MULPM(CH(i,k,2),CH(i-1,k,2),WA(1,i-2),WA(1,i-1),di3,dr3)
      }
  }

NOINLINE static void radb4(size_t ido, size_t l1, const double * restrict cc,
  double * restrict ch, const double * restrict wa)
  {
  const size_t cdim=4;
  static const double sqrt2=1.41421356237309504880;

  for (size_t k=0; k<l1; k++)
    {
    double tr1, tr2;
    PM (tr2,tr1,CC(0,0,k),CC(ido-1,3,k))
    double tr3=2.*CC(ido-1,1,k);
    double tr4=2.*CC(0,2,k);
    PM (CH(0,k,0),CH(0,k,2),tr2,tr3)
    PM (CH(0,k,3),CH(0,k,1),tr1,tr4)
    }
  if ((ido&1)==0)
    for (size_t k=0; k<l1; k++)
      {
      double tr1,tr2,ti1,ti2;
      PM (ti1,ti2,CC(0    ,3,k),CC(0    ,1,k))
      PM (tr2,tr1,CC(ido-1,0,k),CC(ido-1,2,k))
      CH(ido-1,k,0)=tr2+tr2;
      CH(ido-1,k,1)=sqrt2*(tr1-ti1);
      CH(ido-1,k,2)=ti2+ti2;
      CH(ido-1,k,3)=-sqrt2*(tr1+ti1);
      }
  if (ido<=2) return;
  for (size_t k=0; k<l1;++k)
    for (size_t i=2; i<ido; i+=2)
      {
      double ci2, ci3, ci4, cr2, cr3, cr4, ti1, ti2, ti3, ti4, tr1, tr2, tr3, tr4;
      size_t ic=ido-i;
      PM (tr2,tr1,CC(i-1,0,k),CC(ic-1,3,k))
      PM (ti1,ti2,CC(i  ,0,k),CC(ic  ,3,k))
      PM (tr4,ti3,CC(i  ,2,k),CC(ic  ,1,k))
      PM (tr3,ti4,CC(i-1,2,k),CC(ic-1,1,k))
      PM (CH(i-1,k,0),cr3,tr2,tr3)
      PM (CH(i  ,k,0),ci3,ti2,ti3)
      PM (cr4,cr2,tr1,tr4)
      PM (ci2,ci4,ti1,ti4)
      MULPM (CH(i,k,1),CH(i-1,k,1),WA(0,i-2),WA(0,i-1),ci2,cr2)
      MULPM (CH(i,k,2),CH(i-1,k,2),WA(1,i-2),WA(1,i-1),ci3,cr3)
      MULPM (CH(i,k,3),CH(i-1,k,3),WA(2,i-2),WA(2,i-1),ci4,cr4)
      }
  }

NOINLINE static void radb5(size_t ido, size_t l1, const double * restrict cc,
  double * restrict ch, const double * restrict wa)
  {
  const size_t cdim=5;
  static const double tr11= 0.3090169943749474241, ti11=0.95105651629515357212,
                      tr12=-0.8090169943749474241, ti12=0.58778525229247312917;

  for (size_t k=0; k<l1; k++)
    {
    double ti5=CC(0,2,k)+CC(0,2,k);
    double ti4=CC(0,4,k)+CC(0,4,k);
    double tr2=CC(ido-1,1,k)+CC(ido-1,1,k);
    double tr3=CC(ido-1,3,k)+CC(ido-1,3,k);
    CH(0,k,0)=CC(0,0,k)+tr2+tr3;
    double cr2=CC(0,0,k)+tr11*tr2+tr12*tr3;
    double cr3=CC(0,0,k)+tr12*tr2+tr11*tr3;
    double ci4, ci5;
    MULPM(ci5,ci4,ti5,ti4,ti11,ti12)
    PM(CH(0,k,4),CH(0,k,1),cr2,ci5)
    PM(CH(0,k,3),CH(0,k,2),cr3,ci4)
    }
  if (ido==1) return;
  for (size_t k=0; k<l1;++k)
    for (size_t i=2; i<ido; i+=2)
      {
      size_t ic=ido-i;
      double tr2, tr3, tr4, tr5, ti2, ti3, ti4, ti5;
      PM(tr2,tr5,CC(i-1,2,k),CC(ic-1,1,k))
      PM(ti5,ti2,CC(i  ,2,k),CC(ic  ,1,k))
      PM(tr3,tr4,CC(i-1,4,k),CC(ic-1,3,k))
      PM(ti4,ti3,CC(i  ,4,k),CC(ic  ,3,k))
      CH(i-1,k,0)=CC(i-1,0,k)+tr2+tr3;
      CH(i  ,k,0)=CC(i  ,0,k)+ti2+ti3;
      double cr2=CC(i-1,0,k)+tr11*tr2+tr12*tr3;
      double ci2=CC(i  ,0,k)+tr11*ti2+tr12*ti3;
      double cr3=CC(i-1,0,k)+tr12*tr2+tr11*tr3;
      double ci3=CC(i  ,0,k)+tr12*ti2+tr11*ti3;
      double ci4, ci5, cr5, cr4;
      MULPM(cr5,cr4,tr5,tr4,ti11,ti12)
      MULPM(ci5,ci4,ti5,ti4,ti11,ti12)
      double dr2, dr3, dr4, dr5, di2, di3, di4, di5;
      PM(dr4,dr3,cr3,ci4)
      PM(di3,di4,ci3,cr4)
      PM(dr5,dr2,cr2,ci5)
      PM(di2,di5,ci2,cr5)
      MULPM(CH(i,k,1),CH(i-1,k,1),WA(0,i-2),WA(0,i-1),di2,dr2)
      MULPM(CH(i,k,2),CH(i-1,k,2),WA(1,i-2),WA(1,i-1),di3,dr3)
      MULPM(CH(i,k,3),CH(i-1,k,3),WA(2,i-2),WA(2,i-1),di4,dr4)
      MULPM(CH(i,k,4),CH(i-1,k,4),WA(3,i-2),WA(3,i-1),di5,dr5)
      }
  }

#undef CC
#undef CH
#define CC(a,b,c) cc[(a)+ido*((b)+cdim*(c))]
#define CH(a,b,c) ch[(a)+ido*((b)+l1*(c))]
#define C1(a,b,c) cc[(a)+ido*((b)+l1*(c))]
#define C2(a,b) cc[(a)+idl1*(b)]
#define CH2(a,b) ch[(a)+idl1*(b)]

NOINLINE static void radbg(size_t ido, size_t ip, size_t l1,
  double * restrict cc, double * restrict ch, const double * restrict wa,
  const double * restrict csarr)
  {
  const size_t cdim=ip;
  size_t ipph=(ip+1)/ 2;
  size_t idl1 = ido*l1;

  for (size_t k=0; k<l1; ++k)        // 102
    for (size_t i=0; i<ido; ++i)     // 101
      CH(i,k,0) = CC(i,0,k);
  for (size_t j=1, jc=ip-1; j<ipph; ++j, --jc)   // 108
    {
    size_t j2=2*j-1;
    for (size_t k=0; k<l1; ++k)
      {
      CH(0,k,j ) = 2*CC(ido-1,j2,k);
      CH(0,k,jc) = 2*CC(0,j2+1,k);
      }
    }

  if (ido!=1)
    {
    for (size_t j=1, jc=ip-1; j<ipph; ++j,--jc)   // 111
      {
      size_t j2=2*j-1;
      for (size_t k=0; k<l1; ++k)
        for (size_t i=1, ic=ido-i-2; i<=ido-2; i+=2, ic-=2)      // 109
          {
          CH(i  ,k,j ) = CC(i  ,j2+1,k)+CC(ic  ,j2,k);
          CH(i  ,k,jc) = CC(i  ,j2+1,k)-CC(ic  ,j2,k);
          CH(i+1,k,j ) = CC(i+1,j2+1,k)-CC(ic+1,j2,k);
          CH(i+1,k,jc) = CC(i+1,j2+1,k)+CC(ic+1,j2,k);
          }
      }
    }
  for (size_t l=1,lc=ip-1; l<ipph; ++l,--lc)
    {
    for (size_t ik=0; ik<idl1; ++ik)
      {
      C2(ik,l ) = CH2(ik,0)+csarr[2*l]*CH2(ik,1)+csarr[4*l]*CH2(ik,2);
      C2(ik,lc) = csarr[2*l+1]*CH2(ik,ip-1)+csarr[4*l+1]*CH2(ik,ip-2);
      }
    size_t iang=2*l;
    size_t j=3,jc=ip-3;
    for(; j<ipph-3; j+=4,jc-=4)
      {
      iang+=l; if(iang>ip) iang-=ip;
      double ar1=csarr[2*iang], ai1=csarr[2*iang+1];
      iang+=l; if(iang>ip) iang-=ip;
      double ar2=csarr[2*iang], ai2=csarr[2*iang+1];
      iang+=l; if(iang>ip) iang-=ip;
      double ar3=csarr[2*iang], ai3=csarr[2*iang+1];
      iang+=l; if(iang>ip) iang-=ip;
      double ar4=csarr[2*iang], ai4=csarr[2*iang+1];
      for (size_t ik=0; ik<idl1; ++ik)
        {
        C2(ik,l ) += ar1*CH2(ik,j )+ar2*CH2(ik,j +1)
                    +ar3*CH2(ik,j +2)+ar4*CH2(ik,j +3);
        C2(ik,lc) += ai1*CH2(ik,jc)+ai2*CH2(ik,jc-1)
                    +ai3*CH2(ik,jc-2)+ai4*CH2(ik,jc-3);
        }
      }
    for(; j<ipph-1; j+=2,jc-=2)
      {
      iang+=l; if(iang>ip) iang-=ip;
      double ar1=csarr[2*iang], ai1=csarr[2*iang+1];
      iang+=l; if(iang>ip) iang-=ip;
      double ar2=csarr[2*iang], ai2=csarr[2*iang+1];
      for (size_t ik=0; ik<idl1; ++ik)
        {
        C2(ik,l ) += ar1*CH2(ik,j )+ar2*CH2(ik,j +1);
        C2(ik,lc) += ai1*CH2(ik,jc)+ai2*CH2(ik,jc-1);
        }
      }
    for(; j<ipph; ++j,--jc)
      {
      iang+=l; if(iang>ip) iang-=ip;
      double war=csarr[2*iang], wai=csarr[2*iang+1];
      for (size_t ik=0; ik<idl1; ++ik)
        {
        C2(ik,l ) += war*CH2(ik,j );
        C2(ik,lc) += wai*CH2(ik,jc);
        }
      }
    }
  for (size_t j=1; j<ipph; ++j)
    for (size_t ik=0; ik<idl1; ++ik)
      CH2(ik,0) += CH2(ik,j);
  for (size_t j=1, jc=ip-1; j<ipph; ++j,--jc)   // 124
    for (size_t k=0; k<l1; ++k)
      {
      CH(0,k,j ) = C1(0,k,j)-C1(0,k,jc);
      CH(0,k,jc) = C1(0,k,j)+C1(0,k,jc);
      }

  if (ido==1) return;

  for (size_t j=1, jc=ip-1; j<ipph; ++j, --jc)  // 127
    for (size_t k=0; k<l1; ++k)
      for (size_t i=1; i<=ido-2; i+=2)
        {
        CH(i  ,k,j ) = C1(i  ,k,j)-C1(i+1,k,jc);
        CH(i  ,k,jc) = C1(i  ,k,j)+C1(i+1,k,jc);
        CH(i+1,k,j ) = C1(i+1,k,j)+C1(i  ,k,jc);
        CH(i+1,k,jc) = C1(i+1,k,j)-C1(i  ,k,jc);
        }

// All in CH

  for (size_t j=1; j<ip; ++j)
    {
    size_t is = (j-1)*(ido-1);
    for (size_t k=0; k<l1; ++k)
      {
      size_t idij = is;
      for (size_t i=1; i<=ido-2; i+=2)
        {
        double t1=CH(i,k,j), t2=CH(i+1,k,j);
        CH(i  ,k,j) = wa[idij]*t1-wa[idij+1]*t2;
        CH(i+1,k,j) = wa[idij]*t2+wa[idij+1]*t1;
        idij+=2;
        }
      }
    }
  }
#undef C1
#undef C2
#undef CH2

#undef CC
#undef CH
#undef PM
#undef MULPM
#undef WA

static void copy_and_norm(double *c, double *p1, size_t n, double fct)
  {
  if (p1!=c)
    {
    if (fct!=1.)
      for (size_t i=0; i<n; ++i)
        c[i] = fct*p1[i];
    else
      memcpy (c,p1,n*sizeof(double));
    }
  else
    if (fct!=1.)
      for (size_t i=0; i<n; ++i)
        c[i] *= fct;
  }

WARN_UNUSED_RESULT
static int rfftp_forward(rfftp_plan plan, double c[], double fct)
  {
  if (plan->length==1) return 0;
  size_t n=plan->length;
  size_t l1=n, nf=plan->nfct;
  double *ch = RALLOC(double, n);
  if (!ch) return -1;
  double *p1=c, *p2=ch;

  for(size_t k1=0; k1<nf;++k1)
    {
    size_t k=nf-k1-1;
    size_t ip=plan->fct[k].fct;
    size_t ido=n / l1;
    l1 /= ip;
    if(ip==4)
      radf4(ido, l1, p1, p2, plan->fct[k].tw);
    else if(ip==2)
      radf2(ido, l1, p1, p2, plan->fct[k].tw);
    else if(ip==3)
      radf3(ido, l1, p1, p2, plan->fct[k].tw);
    else if(ip==5)
      radf5(ido, l1, p1, p2, plan->fct[k].tw);
    else
      {
      radfg(ido, ip, l1, p1, p2, plan->fct[k].tw, plan->fct[k].tws);
      SWAP (p1,p2,double *);
      }
    SWAP (p1,p2,double *);
    }
  copy_and_norm(c,p1,n,fct);
  DEALLOC(ch);
  return 0;
  }

WARN_UNUSED_RESULT
static int rfftp_backward(rfftp_plan plan, double c[], double fct)
  {
  if (plan->length==1) return 0;
  size_t n=plan->length;
  size_t l1=1, nf=plan->nfct;
  double *ch = RALLOC(double, n);
  if (!ch) return -1;
  double *p1=c, *p2=ch;

  for(size_t k=0; k<nf; k++)
    {
    size_t ip = plan->fct[k].fct,
           ido= n/(ip*l1);
    if(ip==4)
      radb4(ido, l1, p1, p2, plan->fct[k].tw);
    else if(ip==2)
      radb2(ido, l1, p1, p2, plan->fct[k].tw);
    else if(ip==3)
      radb3(ido, l1, p1, p2, plan->fct[k].tw);
    else if(ip==5)
      radb5(ido, l1, p1, p2, plan->fct[k].tw);
    else
      radbg(ido, ip, l1, p1, p2, plan->fct[k].tw, plan->fct[k].tws);
    SWAP (p1,p2,double *);
    l1*=ip;
    }
  copy_and_norm(c,p1,n,fct);
  DEALLOC(ch);
  return 0;
  }

WARN_UNUSED_RESULT
static int rfftp_factorize (rfftp_plan plan)
  {
  size_t length=plan->length;
  size_t nfct=0;
  while ((length%4)==0)
    { if (nfct>=NFCT) return -1; plan->fct[nfct++].fct=4; length>>=2; }
  if ((length%2)==0)
    {
    length>>=1;
    // factor 2 should be at the front of the factor list
    if (nfct>=NFCT) return -1;
    plan->fct[nfct++].fct=2;
    SWAP(plan->fct[0].fct, plan->fct[nfct-1].fct,size_t);
    }
  size_t maxl=(size_t)(sqrt((double)length))+1;
  for (size_t divisor=3; (length>1)&&(divisor<maxl); divisor+=2)
    if ((length%divisor)==0)
      {
      while ((length%divisor)==0)
        {
        if (nfct>=NFCT) return -1;
        plan->fct[nfct++].fct=divisor;
        length/=divisor;
        }
      maxl=(size_t)(sqrt((double)length))+1;
      }
  if (length>1) plan->fct[nfct++].fct=length;
  plan->nfct=nfct;
  return 0;
  }

static size_t rfftp_twsize(rfftp_plan plan)
  {
  size_t twsize=0, l1=1;
  for (size_t k=0; k<plan->nfct; ++k)
    {
    size_t ip=plan->fct[k].fct, ido= plan->length/(l1*ip);
    twsize+=(ip-1)*(ido-1);
    if (ip>5) twsize+=2*ip;
    l1*=ip;
    }
  return twsize;
  }

WARN_UNUSED_RESULT NOINLINE static int rfftp_comp_twiddle (rfftp_plan plan)
  {
  size_t length=plan->length;
  double *twid = RALLOC(double, 2*length);
  if (!twid) return -1;
  sincos_2pibyn_half(length, twid);
  size_t l1=1;
  double *ptr=plan->mem;
  for (size_t k=0; k<plan->nfct; ++k)
    {
    size_t ip=plan->fct[k].fct, ido=length/(l1*ip);
    if (k<plan->nfct-1) // last factor doesn't need twiddles
      {
      plan->fct[k].tw=ptr; ptr+=(ip-1)*(ido-1);
      for (size_t j=1; j<ip; ++j)
        for (size_t i=1; i<=(ido-1)/2; ++i)
          {
          plan->fct[k].tw[(j-1)*(ido-1)+2*i-2] = twid[2*j*l1*i];
          plan->fct[k].tw[(j-1)*(ido-1)+2*i-1] = twid[2*j*l1*i+1];
          }
      }
    if (ip>5) // special factors required by *g functions
      {
      plan->fct[k].tws=ptr; ptr+=2*ip;
      plan->fct[k].tws[0] = 1.;
      plan->fct[k].tws[1] = 0.;
      for (size_t i=1; i<=(ip>>1); ++i)
        {
        plan->fct[k].tws[2*i  ] = twid[2*i*(length/ip)];
        plan->fct[k].tws[2*i+1] = twid[2*i*(length/ip)+1];
        plan->fct[k].tws[2*(ip-i)  ] = twid[2*i*(length/ip)];
        plan->fct[k].tws[2*(ip-i)+1] = -twid[2*i*(length/ip)+1];
        }
      }
    l1*=ip;
    }
  DEALLOC(twid);
  return 0;
  }

NOINLINE static rfftp_plan make_rfftp_plan (size_t length)
  {
  if (length==0) return NULL;
  rfftp_plan plan = RALLOC(rfftp_plan_i,1);
  if (!plan) return NULL;
  plan->length=length;
  plan->nfct=0;
  plan->mem=NULL;
  for (size_t i=0; i<NFCT; ++i)
    plan->fct[i]=(rfftp_fctdata){0,0,0};
  if (length==1) return plan;
  if (rfftp_factorize(plan)!=0) { DEALLOC(plan); return NULL; }
  size_t tws=rfftp_twsize(plan);
  if (tws != 0) {
    plan->mem=RALLOC(double,tws);
    if (!plan->mem) { DEALLOC(plan); return NULL; }
  }
  if (rfftp_comp_twiddle(plan)!=0)
    { DEALLOC(plan->mem); DEALLOC(plan); return NULL; }
  return plan;
  }

NOINLINE static void destroy_rfftp_plan (rfftp_plan plan)
  {
  DEALLOC(plan->mem);
  DEALLOC(plan);
  }
rfftp_plan lw_reuse_fft_create(void) { return make_rfftp_plan(640); }
void lw_reuse_fft_free(rfftp_plan p) { destroy_rfftp_plan(p); }
int lw_reuse_rfft(rfftp_plan p, double data[640]) { return rfftp_forward(p, data, 1.0); }
int lw_reuse_irfft(rfftp_plan p, double data[640]) { return rfftp_backward(p, data, 1.0 / 640.0); }
