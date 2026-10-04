---
id: costs
title: Custos e orçamentos
group: costs
place: costs
tour: costs
---

# Custos e orçamentos

Cada chamada paga que uma geração faz fica registrada com o que custou: o valor que o provedor informou ou o preço da tabela de tarifas do Bardo. [Custos](bardo:go/costs) mostra um mês disso, por provedor, por canal e por vídeo, e guarda um orçamento mensal para cada provedor. [Mostre a tela](bardo:tour/costs).

<a id="month"></a>
## O mês

Custos abre no mês atual. As setas ao lado do nome voltam um mês por vez e avançam de novo até o atual. Os meses seguem o UTC, como os provedores cobram, então uma chamada feita na última noite do mês no Brasil pode contar no mês seguinte.

<a id="spent"></a>
## Quanto o mês gastou

Os números no topo somam tudo: quanto foi gasto e em quantos provedores; quantos orçamentos estão perto ou além do limite; e quantos modelos rodaram sem preço. No Workspace eles são cartões; no Studio, uma linha no cabeçalho.

<a id="budgets"></a>
## Orçamentos

**Orçamentos** lista cada provedor pago: quanto foi usado, quanto foi gasto e o orçamento dele. **Definir orçamento** dá ao provedor um limite mensal em dólares; **Alterar** e **Remover** mudam esse limite.

- **A partir de 80%**: antes de uma geração que levaria o provedor a 80% do orçamento ou mais, a estimativa avisa, e o provedor aparece como **Perto do orçamento**.
- **Em 100%**: uma geração que atingiria o orçamento, ou qualquer uma depois que ele foi atingido, pergunta **Gerar mesmo assim?** antes de começar. Nada começa além de um orçamento sem a sua confirmação, e nada em andamento é interrompido no meio.

Provedores gratuitos, como a YouTube Data API, têm uma cota diária em vez de preço, então não têm orçamento. O orçamento é mensal; o mês seguinte começa do zero.

<a id="rates"></a>
## Tarifas e modelos sem preço

**Tarifas** é a tabela de preços que o Bardo usa quando o provedor não informa quanto a chamada custou: por milhão de tokens, por mil caracteres, por segundo de vídeo ou por hora de áudio. O nome de um modelo vale para todos os modelos que começam com ele. **Editar** muda um preço, **Restaurar** traz de volta o preço do próprio Bardo, e **Adicionar tarifa** dá preço a um modelo que a tabela não tem. Um preço alterado vale para as próximas chamadas; os custos já registrados ficam como estão.

Quando um modelo rodou sem nenhum preço, as chamadas dele contam como zero, e o número **Chamadas sem preço** diz quais modelos. **Adicionar preço** abre o formulário de tarifa com esse modelo preenchido.

<a id="channels"></a>
## Gasto por canal

**Por canal** mostra quanto os vídeos de cada canal custaram no mês, com uma barra comparada ao canal que mais gastou. Chamadas feitas sem canal, como a amostra de voz de uma persona, contam como **Canal desconhecido**.

<a id="videos"></a>
## Os vídeos mais caros

**Vídeos mais caros** lista os projetos de vídeo que mais custaram no mês, com o canal de cada um. Um vídeo que você removeu continua aqui como **Vídeo removido**, porque as chamadas dele foram pagas.
