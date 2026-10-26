---
id: projects
title: Projetos e etapas
group: production
place: projects
tour: projects
---

# Projetos e etapas

Um projeto de vídeo é um vídeo no caminho entre uma ideia aprovada e uma publicação. [Projetos](bardo:go/projects) mostra um projeto por vez: quem narra, as etapas do roteiro à publicação e quanto ele já custou. Aprovar uma ideia em [Temas](themes-ranking.md#review) começa um projeto. [Mostre a tela para mim](bardo:tour/projects).

<a id="switch"></a>
## Escolher um projeto

O nome do projeto abre a tela, com um menu ao lado que lista os outros projetos do canal; escolha um para trocar. O seletor de canal ao lado de **Projetos** lista os projetos de outro canal.

<a id="narrator"></a>
## O narrador

O narrador é a persona que lê a narração do vídeo, e o tom e o estilo de roteiro dela dão forma ao roteiro que o Claude escreve. Um projeto usa a persona padrão do canal até você escolher outra em **Narrador**, só para este vídeo. Mude o padrão do canal em [Canais](bardo:go/channels); crie e ajuste narradores em [Personas](personas.md).

<a id="format"></a>
## Short ou longo

**Formato**, ao lado do nome do projeto, diz se este vídeo é um Short (vertical, menos de um minuto) ou um vídeo longo (16:9, de 8 a 10 minutos). **Do canal** segue o [formato do vídeo](channels.md#format) do canal; escolha **Short** ou **Longo** para fazer só este vídeo do outro tipo.

O formato vale para o que for feito depois: o tamanho do próximo roteiro, o formato das próximas imagens e clipes das cenas, um corte novo no editor e o [preset de render](render.md#targets) do YouTube. O que já foi feito mantém o formato, então, depois de trocar, escreva o roteiro de novo e desenhe as cenas de novo para ter um vídeo coerente.

<a id="mode"></a>
## Como é produzido, e quanto vai custar

**Modo**, ao lado do formato, diz como este vídeo é feito: quais cenas viram clipes de IA e com qual modelo. As outras cenas ficam em imagens paradas, com o [movimento nas imagens](channels.md#defaults) do canal.

- **Econômico**: imagens com movimento, sem clipes de IA. O mais barato.
- **Gancho animado**: imagens com movimento, e as cenas que começam nos primeiros 10 segundos animadas pelo Kling 2.6 em 720p, para prender quem assiste. Canais novos começam assim.
- **Tudo animado**: todas as cenas animadas pelo Kling 2.6 em 720p.
- **Tudo animado, premium**: todas as cenas animadas pelo Kling 3.0 em 1080p.
- **Tudo animado, modelo do canal**: todas as cenas animadas pelo [modelo de vídeo](channels.md#defaults) do canal, como fazem os canais criados antes dos modos.

**Do canal** segue o [modo de produção](channels.md#production) do canal. Cada opção do menu mostra quanto o vídeo inteiro custaria naquele modo, e a linha ao lado mostra o total do modo escolhido e quanto já foi gasto. A estimativa soma o que falta fazer (roteiro, narração, plano de cenas, imagens e retratos, e os clipes que o modo faz) pela [tabela de preços](bardo:go/costs), então ela cai conforme o vídeo é feito. Antes de existir narração, ela supõe um minuto para um Short e oito para um vídeo longo. Com a tabela de preços que vem com o Bardo, um Short inteiro custa cerca de US$ 1,40 no Econômico, US$ 2,20 com o gancho animado, US$ 5,60 tudo animado e US$ 8,10 no premium; um vídeo longo, cerca de US$ 9, US$ 10, US$ 43 e US$ 63. O Kling 2.6 faz clipes de 5 ou 10 segundos, então uma cena de 6 segundos paga por 10.

Um modo cujos clipes precisam de uma chave que você não salvou avisa no menu: os modos do Kling precisam de uma chave Kling em [Configurações › Chaves de API](bardo:go/settings/keys). Trocar o modo vale para o que for animado depois: na etapa Clipes, **Animar cenas sem clipe** (ou **Animar o gancho**) anima só as cenas que o modo anima, clipes já feitos ficam, e qualquer cena ainda pode ser animada à mão com qualquer modelo.

**Teto** é quanto este vídeo pode custar antes de o Bardo perguntar. Um passo pago do vídeo (roteiro, narração, imagens, clipes, takes) que levaria o gasto além do teto pergunta antes, com quanto o passo custa e quanto o resto do vídeo custaria no modo dele; **Gerar mesmo assim** segue. Um passo de preço desconhecido também pergunta. Um job ainda rodando conta para o teto quando termina, então iniciar vários ao mesmo tempo pode passar dele. Deixe vazio para não ter teto.

<a id="stages"></a>
## As etapas

As etapas ficam no topo, em ordem: [Roteiro](script.md), [Narração](narration.md), [Cenas](scenes.md), [Clipes](clips.md), [Edição](editor.md), [Render](render.md) e Publicação. Escolha uma para abri-la; Edição abre o editor. A linha sob cada etapa diz como ela está:

- Quando está pronta, o que ela tem: as palavras do roteiro, a duração da narração, as imagens ou clipes feitos.
- **Gerando…** enquanto uma tarefa dela roda. Acompanhe em [Tarefas](bardo:go/jobs).
- **… para revisar** quando uma nova versão espera ao lado da atual.
- **Desatualizada** quando uma etapa anterior mudou depois que ela foi feita.
- **Depois do …** ou **Depois da …** quando ela está bloqueada (abaixo).

<a id="unlock"></a>
## Como uma etapa se abre

Cada etapa trabalha sobre o que a anterior lhe deu, então ela fica bloqueada até lá, e a linha dela diz o que espera:

- **Narração** se abre quando há um roteiro, porque o narrador o lê.
- **Cenas** se abre quando há uma narração, porque as cenas são cronometradas pelas palavras dela.
- **Clipes** se abre quando uma cena tem imagem, porque o clipe a anima.
- **Edição** se abre quando as cenas estão planejadas: o editor monta um primeiro corte a partir delas e da narração.
- **Render** e **Publicação** se abrem depois da edição e do render.

Nada é jogado fora quando uma etapa anterior muda. O que foi feito a partir dela aparece como **Desatualizada**, para você saber o que refazer: uma narração depois que o roteiro mudou, as cenas depois que a narração foi refeita.

<a id="cost"></a>
## Quanto já custou

A linha sob o nome do projeto mostra o nicho, quando ele começou e, assim que algo for pago, quanto o projeto já custou: cada chamada a um provedor para este vídeo, do roteiro aos clipes. Cada geração mostra a estimativa antes de você começar, e uma chamada que passaria de um orçamento ou do [teto](#mode) do vídeo pergunta antes. [Custos](bardo:go/costs) separa os gastos por provedor, canal e vídeo; veja [Suas chaves de API](api-keys.md#budgets) para os orçamentos.
